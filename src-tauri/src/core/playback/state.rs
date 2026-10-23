use crate::core::errors::PlaybackErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Idle,
    Resolving,
    Preparing,
    Ready,
    Playing,
    Paused,
    Buffering,
    Blocked,
    Ended,
    Error,
}

// Exhaustive on `from`: a new state fails to compile until its row is written.
// Two cross-cutting rules: any state may move to Error, and any state may move to Resolving
// (Load replaces the session and cancels the outstanding request). Stop cancels an in-flight
// request, so Resolving, Preparing and Buffering may move to Idle.
pub fn validate_transition(
    from: PlaybackState,
    to: PlaybackState,
) -> Result<(), PlaybackErrorKind> {
    use PlaybackState::*;
    let ok = to == Error
        || to == Resolving
        || match from {
            Idle => false,
            Resolving => matches!(to, Preparing | Idle),
            Preparing => matches!(to, Ready | Idle),
            Ready => matches!(to, Playing | Blocked | Idle),
            Playing => matches!(to, Paused | Buffering | Ended | Blocked | Idle),
            Paused => matches!(to, Playing | Blocked | Idle),
            Buffering => matches!(to, Playing | Paused | Blocked | Idle),
            Blocked | Ended | Error => to == Idle,
        };
    if ok {
        Ok(())
    } else {
        Err(PlaybackErrorKind::InvalidTransition { from, to })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PlaybackState::*;

    const ALL: [PlaybackState; 10] = [
        Idle, Resolving, Preparing, Ready, Playing, Paused, Buffering, Blocked, Ended, Error,
    ];

    const ALLOWED: [(PlaybackState, PlaybackState); 32] = [
        (Ready, Blocked),
        (Playing, Blocked),
        (Paused, Blocked),
        (Buffering, Blocked),
        (Buffering, Paused),
        (Idle, Resolving),
        (Resolving, Preparing),
        (Preparing, Ready),
        (Ready, Playing),
        (Playing, Paused),
        (Playing, Buffering),
        (Playing, Ended),
        (Paused, Playing),
        (Buffering, Playing),
        (Ready, Idle),
        (Playing, Idle),
        (Paused, Idle),
        (Ended, Idle),
        (Blocked, Idle),
        (Error, Idle),
        (Resolving, Idle),
        (Preparing, Idle),
        (Buffering, Idle),
        (Resolving, Resolving),
        (Preparing, Resolving),
        (Ready, Resolving),
        (Playing, Resolving),
        (Paused, Resolving),
        (Buffering, Resolving),
        (Blocked, Resolving),
        (Ended, Resolving),
        (Error, Resolving),
    ];

    #[test]
    fn load_to_resolving_from_settled_states_is_ok() {
        for from in [Ready, Playing, Paused, Error, Ended] {
            assert_eq!(validate_transition(from, Resolving), Ok(()), "{from:?}");
        }
    }

    #[test]
    fn preparing_to_idle_is_ok() {
        assert_eq!(validate_transition(Preparing, Idle), Ok(()));
    }

    #[test]
    fn listed_transitions_are_ok() {
        for (from, to) in ALLOWED {
            assert_eq!(validate_transition(from, to), Ok(()), "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn every_pair_not_listed_is_invalid_except_to_error() {
        for from in ALL {
            for to in ALL {
                let expected = if to == Error || ALLOWED.contains(&(from, to)) {
                    Ok(())
                } else {
                    Err(PlaybackErrorKind::InvalidTransition { from, to })
                };
                assert_eq!(
                    validate_transition(from, to),
                    expected,
                    "{from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn idle_to_playing_is_invalid_transition() {
        assert_eq!(
            validate_transition(Idle, Playing),
            Err(PlaybackErrorKind::InvalidTransition {
                from: Idle,
                to: Playing
            })
        );
    }

    #[test]
    fn ended_to_playing_is_invalid() {
        assert!(validate_transition(Ended, Playing).is_err());
    }

    #[test]
    fn error_to_playing_is_invalid() {
        assert!(validate_transition(Error, Playing).is_err());
    }

    #[test]
    fn blocked_to_playing_is_invalid() {
        assert!(validate_transition(Blocked, Playing).is_err());
    }

    #[test]
    fn every_state_can_move_to_error() {
        for from in ALL {
            assert_eq!(
                validate_transition(from, Error),
                Ok(()),
                "{from:?} -> Error"
            );
        }
    }
}
