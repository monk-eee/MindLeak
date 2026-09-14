use std::time::Duration;

use tokio::{sync::oneshot, task::JoinHandle};

pub struct RunningServer {
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl RunningServer {
    pub fn new(shutdown: oneshot::Sender<()>, task: JoinHandle<()>) -> Self {
        Self {
            shutdown: Some(shutdown),
            task: Some(task),
        }
    }

    pub async fn stop(mut self) {
        self.shutdown
            .take()
            .unwrap()
            .send(())
            .expect("fixture service is still running");
        tokio::time::timeout(Duration::from_secs(5), self.task.as_mut().unwrap())
            .await
            .expect("fixture requests must drain within five seconds")
            .expect("fixture service must not panic");
        self.task.take();
    }
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
