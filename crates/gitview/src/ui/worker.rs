//! Runs blocking work (network calls) off the UI thread and hands the result back to it.

use std::{cell::RefCell, sync::mpsc, thread};

use wxdragon::prelude::{call_after, wake_up_idle};

type Poll = Box<dyn FnMut() -> Option<Box<dyn FnOnce()>>>;

thread_local! {
	static PENDING: RefCell<Vec<Poll>> = const { RefCell::new(Vec::new()) };
}

/// Runs `job` on a new thread, then `done` with its result on the UI thread.
pub fn spawn<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static, done: impl FnOnce(T) + 'static) {
	let (sender, receiver) = mpsc::channel();
	let mut done = Some(done);
	PENDING.with_borrow_mut(|pending| {
		pending.push(Box::new(move || {
			let value = receiver.try_recv().ok()?;
			let done = done.take()?;
			Some(Box::new(move || done(value)))
		}));
	});
	thread::spawn(move || {
		let _ = sender.send(job());
		call_after(Box::new(deliver));
		wake_up_idle();
	});
}

// One result at a time, with PENDING released before each `done` runs: a `done` that opens a
// modal dialog pumps events, and results that arrive meanwhile must still be reachable.
fn deliver() {
	while let Some(finish) = PENDING.with_borrow_mut(|pending| {
		let (index, finish) = pending.iter_mut().enumerate().find_map(|(index, poll)| Some((index, poll()?)))?;
		drop(pending.swap_remove(index));
		Some(finish)
	}) {
		finish();
	}
}
