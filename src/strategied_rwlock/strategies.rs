extern crate alloc;
use alloc::{boxed::Box, vec, vec::Vec};

use super::{Method, State, StrategyInput, StrategyResult};

pub fn fair(entries: StrategyInput) -> StrategyResult {
    struct CombinedState {
        collection: Vec<State>,
        future_read: State,
        future_write: State,
    }

    let mut state = CombinedState {
        collection: vec![],
        future_read: State::Ok,
        future_write: State::Ok,
    };

    entries.for_each(|(_handle_id, method)| match method {
        Method::Read => {
            state.collection.push(state.future_read);
            state.future_write = State::Blocked;
        }
        Method::Write => {
            state.collection.push(state.future_write);
            state.future_read = State::Blocked;
            state.future_write = State::Blocked;
        }
    });

    Box::new(state.collection.into_iter())
}
