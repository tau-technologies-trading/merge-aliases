use std::fs::File;
use std::io::{self, Write};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};

enum Message {
    Write(Vec<u8>),
    Flush(mpsc::SyncSender<io::Result<()>>),
}

pub(crate) struct QueuedWriter {
    sender: Option<SyncSender<Message>>,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl QueuedWriter {
    pub(crate) fn new(mut file: File) -> Self {
        let (sender, receiver) = mpsc::sync_channel(8);
        let worker = thread::spawn(move || {
            while let Ok(message) = receiver.recv() {
                match message {
                    Message::Write(bytes) => file.write_all(&bytes)?,
                    Message::Flush(reply) => {
                        let result = file.flush();
                        let failed = result.is_err();
                        let _ = reply.send(result);
                        if failed {
                            return Err(io::Error::other("queued file flush failed"));
                        }
                    }
                }
            }
            file.flush()
        });
        Self {
            sender: Some(sender),
            worker: Some(worker),
        }
    }

    pub(crate) fn finish(mut self) -> io::Result<()> {
        self.sender.take();
        self.worker
            .take()
            .expect("queued writer worker exists")
            .join()
            .map_err(|_| io::Error::other("queued file writer panicked"))?
    }
}

impl Write for QueuedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        self.sender
            .as_ref()
            .expect("queued writer is active")
            .send(Message::Write(bytes.to_vec()))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "queued file writer stopped"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let (reply, result) = mpsc::sync_channel(0);
        self.sender
            .as_ref()
            .expect("queued writer is active")
            .send(Message::Flush(reply))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "queued file writer stopped"))?;
        result
            .recv()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "queued file writer stopped"))?
    }
}

impl Drop for QueuedWriter {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
