use std::iter;

use powerlocks::rwlock::{State, StrategyInput, StrategyResult};

pub fn broken_always_allow(entries: StrategyInput) -> StrategyResult {
    Box::new(entries.map(|_| State::Ok))
}

pub fn broken_block_on_second(entries: StrategyInput) -> StrategyResult {
    let len = entries.count();
    let state = if len >= 2 { State::Blocked } else { State::Ok };
    Box::new(iter::repeat_n(state, len))
}
