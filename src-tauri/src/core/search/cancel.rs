use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// Shared flag: clones observe the same cancellation.
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_token_is_not_cancelled() {
        assert!(!CancelToken::new().is_cancelled());
    }

    #[test]
    fn cancel_is_visible_through_clone() {
        let a = CancelToken::new();
        let b = a.clone();
        a.cancel();
        assert!(b.is_cancelled());
    }

    #[test]
    fn cancelling_twice_is_harmless() {
        let t = CancelToken::new();
        t.cancel();
        t.cancel();
        assert!(t.is_cancelled());
    }
}
