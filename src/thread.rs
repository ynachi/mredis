//! Specialized thread pool for our application. So everything is tailored.
//! We are not trying to build a general purpose thread pool.

use std::sync::mpsc::TryRecvError;
use std::sync::{mpsc, Arc, Mutex, TryLockError};
use std::thread;
use tracing::{debug, error};

type Job = Box<dyn FnOnce() + 'static + Send>;

pub struct Pool {
    workers: Vec<Worker>,
    sender: mpsc::SyncSender<Job>,
}

#[derive(Debug)]
struct Worker {
    handler: thread::JoinHandle<()>,
    id: usize,
}

impl Worker {
    fn new(receiver: Arc<Mutex<mpsc::Receiver<Job>>>, id: usize) -> Self {
        // Workers are created at application initialization.
        // Creating them all is a precondition, so failing to do so
        // is a fatal error.
        let handler = thread::Builder::new()
            .spawn(move || Self::process_jobs(receiver))
            .expect("failed to create thread");
        Worker { handler, id }
    }

    fn process_jobs(receiver: Arc<Mutex<mpsc::Receiver<Job>>>) {
        loop {
            // @TODO: benchmark and change if needed
            let guard = receiver.try_lock();
            match guard {
                Ok(recv) => {
                    match recv.try_recv() {
                        Ok(job) => {
                            debug!("worker received job");
                            job();
                        }
                        Err(TryRecvError::Empty) => continue,
                        Err(e) => {
                            // @TODO: The whole pool should shut down in this case
                            error!("worker queue unexpectedly closed: {}", e);
                            return;
                        }
                    }
                }
                Err(e) => match e {
                    TryLockError::WouldBlock => continue,
                    _ => {
                        debug!("failed to acquire lock: {}", e);
                    }
                },
            }
        }
    }
}

impl Pool {
    pub fn new(worker_count: usize, max_queue_length: usize) -> Self {
        assert!(worker_count > 0, "cannot create an empty thread pool");

        let mut workers = Vec::with_capacity(worker_count);
        let (sender, receiver) = mpsc::sync_channel(max_queue_length);
        let receiver = Arc::new(Mutex::new(receiver));

        for i in 0..worker_count {
            workers.push(Worker::new(receiver.clone(), i));
        }

        Pool { workers, sender }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        if let Err(e) = self.sender.send(job) {
            error!("worker queue unexpectedly closed: {}", e);
            // @TODO: exit the whole pool as the channel is closed
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        debug!("Shutting down all workers.");

        while let Some(worker) = self.workers.pop() {
            debug!("Shutting down worker {}", worker.id);

            if let Err(error) = worker.handler.join() {
                debug!("Error when joining worker {}: {:?}", worker.id, error);
            }
        }
    }
}
