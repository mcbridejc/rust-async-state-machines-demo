use core::pin::Pin;

/// A wrapper struct to execute a future one call at a time
pub struct AsyncStateMachine<'a, F, T>
where
    F: Future<Output = T>
{
    fut: Pin<&'a mut F>,
}

impl<'a, F, T> AsyncStateMachine<'a, F, T>
where
    F: Future<Output = T>
{
    /// Create a new state machine from a provided future
    ///
    /// `fut` must be pinned. This can be achieved using either `Box::pin` to pin on the heap, or
    /// the `pin!` macro to pin on the stack.
    pub fn new(fut: Pin<&'a mut F>) -> Self {
        Self { fut: fut }
    }

    /// Poll the future one time
    ///
    /// If the future completes, Some(T) is returned with the returned value. If the future is still
    /// pending, then None is returned
    pub fn exec(&mut self) -> Option<T> {
        poll_once(self.fut.as_mut())
    }
}

/// Poll a future one time, and return its result if it completes
pub fn poll_once<T>(mut f: Pin<&mut dyn Future<Output = T>>) -> Option<T> {
    let mut cx = futures::task::Context::from_waker(futures::task::noop_waker_ref());

    match f.as_mut().poll(&mut cx) {
        core::task::Poll::Ready(result) => Some(result),
        core::task::Poll::Pending => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::pending;
    use core::pin::pin;

    #[test]
    fn test_poll_once() {
        let mut future = pin!(async {
            let mut i = 0;
            loop {
                if i == 2 {
                    return 42;
                } else {
                    i += 1;
                    pending!()
                }
            }
        });

        // i = 0. Not done.
        assert_eq!(poll_once(future.as_mut()), None);
        // i = 1. Not done.
        assert_eq!(poll_once(future.as_mut()), None);
        // i = 2. Done!
        assert_eq!(poll_once(future.as_mut()), Some(42));
    }
}