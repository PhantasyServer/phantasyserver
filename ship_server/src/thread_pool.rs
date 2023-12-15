use crate::{Action, Error};
use parking_lot::Mutex;
use std::{
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};

type Return = Result<Action, Error>;
type Job = Box<dyn FnOnce() -> Return + Send + 'static>;

#[allow(dead_code)]
pub struct ThreadPool {
    lastid: usize,
    workers: Vec<Worker>,
    sender: Option<mpsc::SyncSender<(usize, Job)>>,
    recv: Arc<Mutex<mpsc::Receiver<(usize, Job)>>>,
    result_send: mpsc::Sender<(usize, Return)>,
}

impl ThreadPool {
    pub fn new(size: usize, result_send: mpsc::Sender<(usize, Return)>) -> ThreadPool {
        assert!(size > 0);
        let (send, recv) = mpsc::sync_channel(size * 2);
        let recv = Arc::new(Mutex::new(recv));
        let mut workers = Vec::with_capacity(size);
        for id in 0..size {
            workers.push(Worker::new(id, recv.clone(), result_send.clone()));
        }
        ThreadPool {
            lastid: size,
            workers,
            sender: Some(send),
            recv,
            result_send,
        }
    }
    #[allow(dead_code)]
    pub fn add(&mut self, count: usize) {
        for id in self.lastid..self.lastid + count {
            self.workers
                .push(Worker::new(id, self.recv.clone(), self.result_send.clone()));
        }
        self.lastid += count;
    }
    pub fn exec<F>(&self, pos: usize, f: F)
    where
        F: FnOnce() -> Return + Send + 'static,
    {
        let job = Box::new(f);
        self.sender.as_ref().unwrap().send((pos, job)).unwrap();
    }
}

#[allow(dead_code)]
struct Worker {
    id: usize,
    _thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(
        id: usize,
        receiver: Arc<Mutex<mpsc::Receiver<(usize, Job)>>>,
        sender: mpsc::Sender<(usize, Return)>,
    ) -> Worker {
        let thread = thread::spawn(move || loop {
            let msg = receiver.lock().recv();
            match msg {
                Ok((pos, job)) => {
                    let result = job();
                    if let Ok(Action::Nothing) = result {
                    } else {
                        sender.send((pos, result)).unwrap();
                    }
                }
                Err(_) => break,
            }
            thread::sleep(Duration::from_millis(10));
        });
        Worker {
            id,
            _thread: Some(thread),
        }
    }
}
