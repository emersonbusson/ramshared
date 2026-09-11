use std::collections::VecDeque;

/// Persists time-series data for trend analysis.
#[derive(Clone, Debug, Default)]
pub struct MonitorHistory {
    data: VecDeque<u64>,
    limit: usize,
}

impl MonitorHistory {
    /// Creates a new bounded history accumulator.
    pub fn new(limit: usize) -> Self {
        Self {
            data: VecDeque::with_capacity(limit),
            limit,
        }
    }

    /// Records an observation, evicting the oldest if at capacity.
    pub fn push(&mut self, value: u64) {
        self.data.push_back(value);
        while self.data.len() > self.limit {
            self.data.pop_front();
        }
    }

    /// Returns a snapshot of the current history.
    pub fn values(&self) -> Vec<u64> {
        self.data.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_history() {
        let mut history = MonitorHistory::new(3);
        history.push(10);
        history.push(20);
        history.push(30);
        assert_eq!(history.values(), vec![10, 20, 30]);
        history.push(40);
        assert_eq!(history.values(), vec![20, 30, 40]);
    }
}
