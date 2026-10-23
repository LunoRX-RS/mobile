pub struct QuotaBudget {
    pub(crate) limit_per_day: u32,
    pub(crate) used: u32,
    pub(crate) day: u64,
}

impl QuotaBudget {
    // limit 0 = deny every call.
    pub fn new(limit_per_day: u32) -> Self {
        Self {
            limit_per_day,
            used: 0,
            day: 0,
        }
    }

    // shortcut: caller passes the Pacific-time day number; no tz math here, upgrade when the engine owns a clock
    pub fn try_consume(&mut self, today: u64) -> bool {
        if today != self.day {
            self.day = today;
            self.used = 0;
        }
        if self.used < self.limit_per_day {
            self.used += 1;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_two_allows_two_per_day_then_denies() {
        let mut q = QuotaBudget::new(2);
        assert!(q.try_consume(5));
        assert!(q.try_consume(5));
        assert!(!q.try_consume(5));
    }

    #[test]
    fn new_day_resets_budget_to_one_used_not_carried_over() {
        let mut q = QuotaBudget::new(2);
        assert!(q.try_consume(5));
        assert!(q.try_consume(5));
        assert!(!q.try_consume(5));
        assert!(q.try_consume(6));
        assert_eq!(q.used, 1);
        assert!(q.try_consume(6));
        assert!(!q.try_consume(6));
    }

    #[test]
    fn limit_zero_always_denies() {
        let mut q = QuotaBudget::new(0);
        assert!(!q.try_consume(5));
        assert!(!q.try_consume(6));
        assert_eq!(q.used, 0);
    }

    #[test]
    fn exhausted_budget_does_not_reset_on_same_day() {
        let mut q = QuotaBudget::new(1);
        assert!(q.try_consume(5));
        assert!(!q.try_consume(5));
        assert!(!q.try_consume(5));
    }
}
