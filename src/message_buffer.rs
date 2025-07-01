use std::sync::mpsc::Receiver;

pub struct MessageBuffer<'a, T> {
    counter: usize,
    buffer: Vec<T>,
    channel: &'a Receiver<T>,
}

// todo: this needs to be async
impl<T> MessageBuffer<'_, T> {
    pub fn new(channel: &Receiver<T>) -> MessageBuffer<T> {
        MessageBuffer {
            counter: 0,
            buffer: vec![],
            channel,
        }
    }

    pub fn poll(&mut self) -> T {
        self.fill();

        // if there's anything in the buffer, pop
        if !self.buffer.is_empty() {
            return self.buffer.pop().unwrap();
        }

        // otherwise, wait around for a new message first
        self.buffer.push(self.channel.recv().unwrap());

        self.poll()
    }

    pub fn get_final_count(&mut self) -> usize {
        self.fill();

        if self.buffer.is_empty() {
            let ret = self.counter;
            self.counter = 0;
            return ret;
        }

        0
    }

    pub fn inc(&mut self) {
        self.counter += 1;
    }

    fn fill(&mut self) {
        while let Ok(message) = self.channel.try_recv() {
            self.buffer.push(message)
        }
    }
}
