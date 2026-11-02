//! `Display` text for the engine's answer errors. `Engine::answer` sends this
//! text to the Seat that answered, so it names no object id or card. See
//! `DESIGN.md`.

use std::fmt;

use super::{
    ActionError, AmplifyChoiceError, AttachChoiceError, BottomCardsError, CascadeChoiceError,
    CommanderReturnChoiceError, CopyChoiceError, DevourChoiceError, DiscardChoiceError,
    DiscardError, LegendRuleChoiceError, MadnessChoiceError, ModeChoiceError, MulliganError,
    PaymentError, ReplacementChoiceError, ScriptChoiceError, ScryChoiceError, SearchChoiceError,
    SurveilChoiceError, TriggerModeChoiceError, TriggerOrderError, TriggerTargetsError,
    UnlessPaidError,
};

// The facade confirms the open pause and the player first, so the
// `NotAwaiting*`, `WrongPlayer` and `NotCurrentPlayer` texts only reach a
// caller that uses a `submit_*` method directly.
macro_rules! unit_text {
    ($error:ident { $($variant:ident => $text:literal,)+ }) => {
        impl fmt::Display for $error {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(match self {
                    $($error::$variant => $text,)+
                })
            }
        }
    };
}

// TODO: IllegalAction strings still use engine terms such as "actor" and method names
impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActionError::NotYourTurn => f.write_str("it is not your priority"),
            ActionError::IllegalAction(reason) => f.write_str(reason),
            ActionError::Rejected(rejected) => rejected.fmt(f),
        }
    }
}

impl fmt::Display for MadnessChoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MadnessChoiceError::NotAwaitingChoice => {
                f.write_str("no madness cast is waiting for an answer")
            }
            MadnessChoiceError::NoMadnessCost => f.write_str("this card has no madness cost"),
            MadnessChoiceError::IllegalCast(rejected) => rejected.fmt(f),
            MadnessChoiceError::Internal => f.write_str("the engine hit an internal error"),
        }
    }
}

impl fmt::Display for CascadeChoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CascadeChoiceError::NotAwaitingChoice => {
                f.write_str("no cascade cast is waiting for an answer")
            }
            CascadeChoiceError::NoCandidate => {
                f.write_str("cascade found no card to cast, so the only answer is to decline")
            }
            CascadeChoiceError::IllegalCast(rejected) => rejected.fmt(f),
            CascadeChoiceError::Internal => f.write_str("the engine hit an internal error"),
        }
    }
}

impl fmt::Display for DiscardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscardError::NotAwaitingDiscard => {
                f.write_str("no cleanup discard is waiting for an answer")
            }
            DiscardError::WrongCount { expected, got } => {
                write!(f, "expected {expected} card(s) to discard, got {got}")
            }
            DiscardError::DuplicateCard => f.write_str("the same card was chosen twice"),
            DiscardError::CardNotInHand => f.write_str("a chosen card is not in your hand"),
        }
    }
}

impl fmt::Display for BottomCardsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BottomCardsError::NotAwaitingChoice => {
                f.write_str("no cards are waiting to go to the bottom of a library")
            }
            BottomCardsError::WrongPlayer => {
                f.write_str("another player is choosing cards to put on the bottom")
            }
            BottomCardsError::WrongCount { expected, got } => {
                write!(
                    f,
                    "expected {expected} card(s) to put on the bottom, got {got}"
                )
            }
            BottomCardsError::CardNotInHand => f.write_str("a chosen card is not in your hand"),
            BottomCardsError::DuplicateCard => f.write_str("the same card was chosen twice"),
        }
    }
}

unit_text!(ReplacementChoiceError {
    NotAwaitingChoice => "no replacement effect choice is waiting for an answer",
    InvalidCandidate => "that replacement effect is not one of the choices",
});

unit_text!(LegendRuleChoiceError {
    NotAwaitingChoice => "no legend rule choice is waiting for an answer",
    InvalidCandidate => "that permanent is not one of the legends to keep",
});

unit_text!(AttachChoiceError {
    NotAwaitingChoice => "no attach choice is waiting for an answer",
    InvalidCandidate => "that is not one of the objects it can attach to",
});

unit_text!(DiscardChoiceError {
    NotAwaitingChoice => "no discard choice is waiting for an answer",
    NotCurrentPlayer => "only the discarding player may choose the card",
    InvalidCandidate => "that card is not in the discarding player's hand",
});

unit_text!(DevourChoiceError {
    NotAwaitingChoice => "no devour choice is waiting for an answer",
    InvalidCandidate => "a chosen creature cannot be sacrificed for devour",
    DuplicateCandidate => "the same creature was chosen twice",
});

unit_text!(AmplifyChoiceError {
    NotAwaitingChoice => "no amplify choice is waiting for an answer",
    InvalidCandidate => "a chosen card cannot be revealed for amplify",
    DuplicateCandidate => "the same card was chosen twice",
});

unit_text!(UnlessPaidError {
    NotAwaitingChoice => "no unless-paid cost is waiting for an answer",
    NotCurrentPayer => "another player is deciding whether to pay",
    InvalidPlan => "the payment does not cover the cost",
});

unit_text!(CopyChoiceError {
    NotAwaitingChoice => "no copy choice is waiting for an answer",
    InvalidCandidate => "that is not one of the objects to copy",
});

unit_text!(ModeChoiceError {
    NotAwaitingChoice => "no mode choice is waiting for an answer",
    InvalidCount => "the number of chosen modes is outside the allowed range",
    DuplicateMode => "the same mode was chosen twice",
    NoSuchMode => "a chosen mode does not exist on this card",
    UnsupportedModeCombination => "those modes cannot be chosen together",
});

unit_text!(CommanderReturnChoiceError {
    NotAwaitingChoice => "no commander return choice is waiting for an answer",
});

unit_text!(ScriptChoiceError {
    NotAwaitingChoice => "no script choice is waiting for an answer",
    InvalidCandidate => "that choice does not match what the card asks for",
    WrongRequestKind => "this choice asks for a different kind of answer",
});

unit_text!(TriggerOrderError {
    NotAwaitingChoice => "no trigger order is waiting for an answer",
    WrongLength => "the order lists the wrong number of triggers",
    NotAPermutation => "the order must list each waiting trigger once",
});

unit_text!(TriggerTargetsError {
    NotAwaitingChoice => "no trigger targets are waiting for an answer",
    InvalidTargets => "the chosen targets are not legal for this ability",
});

unit_text!(TriggerModeChoiceError {
    NotAwaitingChoice => "no trigger mode choice is waiting for an answer",
    InvalidCount => "the number of chosen modes is outside the allowed range",
    DuplicateMode => "the same mode was chosen twice",
    NoSuchMode => "a chosen mode does not exist on this ability",
    UnsupportedModeCombination => "those modes cannot be chosen together",
});

unit_text!(PaymentError {
    NotAwaitingPayment => "no payment is waiting for an answer",
    InvalidPlan => "the payment does not cover the cost",
});

unit_text!(ScryChoiceError {
    NotAwaitingChoice => "no scry is waiting for an answer",
    WrongCards => "the two piles must hold each scried card exactly once",
});

unit_text!(SurveilChoiceError {
    NotAwaitingChoice => "no surveil is waiting for an answer",
    WrongCards => "the two piles must hold each surveilled card exactly once",
});

unit_text!(SearchChoiceError {
    NotAwaitingChoice => "no search is waiting for an answer",
    TooMany => "more cards were chosen than the search allows",
    NotACandidate => "a chosen card is not one of the search results, or was chosen twice",
});

unit_text!(MulliganError {
    NotAwaitingDecision => "no mulligan decision is waiting for an answer",
    WrongPlayer => "another player is deciding on a mulligan",
    MaxMulligansReached => "you have taken the most mulligans allowed and must keep",
    InvalidAction => "only keep or mulligan is allowed here",
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Rejected;

    #[test]
    fn an_illegal_action_shows_its_own_reason() {
        let reason = || Rejected::IllegalAction("x".to_string());
        assert_eq!(reason().to_string(), "x");
        assert_eq!(ActionError::Rejected(reason()).to_string(), "x");
        assert_eq!(ActionError::IllegalAction("x".to_string()).to_string(), "x");
    }

    #[test]
    fn a_wrapped_invalid_target_shows_the_target_sentence() {
        let sentence = "a chosen target is not legal";
        assert_eq!(Rejected::InvalidTarget.to_string(), sentence);
        assert_eq!(
            MadnessChoiceError::IllegalCast(Rejected::InvalidTarget).to_string(),
            sentence
        );
        assert_eq!(
            CascadeChoiceError::IllegalCast(Rejected::InvalidTarget).to_string(),
            sentence
        );
    }

    #[test]
    fn a_wrong_count_shows_both_numbers() {
        let error = BottomCardsError::WrongCount {
            expected: 2,
            got: 1,
        };
        assert_eq!(
            error.to_string(),
            "expected 2 card(s) to put on the bottom, got 1"
        );
    }
}
