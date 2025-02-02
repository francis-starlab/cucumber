// Copyright (c) 2018-2022  Brendan Molloy <brendan@bbqsrc.net>,
//                          Ilya Solovyiov <ilya.solovyiov@gmail.com>,
//                          Kai Ren <tyranron@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Key occurrences in a lifecycle of [Cucumber] execution.
//!
//! The top-level enum here is [`Cucumber`].
//!
//! Each event enum contains variants indicating what stage of execution
//! [`Runner`] is at, and variants with detailed content about the precise
//! sub-event.
//!
//! [`Runner`]: crate::Runner
//! [Cucumber]: https://cucumber.io

use std::{any::Any, fmt, sync::Arc};

#[cfg(feature = "timestamps")]
use std::time::SystemTime;

use derive_more::{AsRef, Deref, DerefMut, Display, Error, From};

use crate::{step, writer::basic::coerce_error};

/// Alias for a [`catch_unwind()`] error.
///
/// [`catch_unwind()`]: std::panic::catch_unwind()
pub type Info = Arc<dyn Any + Send + 'static>;

/// Arbitrary event, optionally paired with additional metadata.
///
/// Any metadata is added by enabling the correspondent library feature:
/// - `timestamps`: adds time of when this [`Event`] has happened.
#[derive(AsRef, Clone, Copy, Debug, Deref, DerefMut)]
#[non_exhaustive]
pub struct Event<T: ?Sized> {
    /// [`SystemTime`] when this [`Event`] has happened.
    #[cfg(feature = "timestamps")]
    pub at: SystemTime,

    /// Actual value of this [`Event`].
    #[as_ref]
    #[deref]
    #[deref_mut]
    pub value: T,
}

impl<T> Event<T> {
    /// Creates a new [`Event`] out of the given `value`.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            #[cfg(feature = "timestamps")]
            at: SystemTime::now(),
            value,
        }
    }

    /// Unwraps the inner [`Event::value`] loosing all the attached metadata.
    #[allow(clippy::missing_const_for_fn)] // false positive: drop in const
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }

    /// Splits this [`Event`] to the inner [`Event::value`] and its detached
    /// metadata.
    #[must_use]
    pub fn split(self) -> (T, Metadata) {
        self.replace(())
    }

    /// Replaces the inner [`Event::value`] with the given one, dropping the old
    /// one in place.
    #[must_use]
    pub fn insert<V>(self, value: V) -> Event<V> {
        self.replace(value).1
    }

    /// Maps the inner [`Event::value`] with the given function.
    #[must_use]
    pub fn map<V>(self, f: impl FnOnce(T) -> V) -> Event<V> {
        let (val, meta) = self.split();
        meta.insert(f(val))
    }

    /// Replaces the inner [`Event::value`] with the given one, returning the
    /// old one along.
    #[allow(clippy::missing_const_for_fn)] // false positive: drop in const
    #[must_use]
    pub fn replace<V>(self, value: V) -> (T, Event<V>) {
        let event = Event {
            #[cfg(feature = "timestamps")]
            at: self.at,
            value,
        };
        (self.value, event)
    }
}

/// Shortcut for a detached metadata of an arbitrary [`Event`].
pub type Metadata = Event<()>;

impl Metadata {
    /// Wraps the given `value` with this [`Event`] metadata.
    #[must_use]
    pub fn wrap<V>(self, value: V) -> Event<V> {
        self.replace(value).1
    }
}

/// Top-level [Cucumber] run event.
///
/// [Cucumber]: https://cucumber.io
#[derive(Debug)]
pub enum Cucumber<World> {
    /// [`Cucumber`] execution being started.
    Started,

    /// [`Feature`] event.
    Feature(Arc<gherkin::Feature>, Feature<World>),

    /// [`Cucumber`] execution being finished.
    Finished,
}

// Manual implementation is required to omit the redundant `World: Clone` trait
// bound imposed by `#[derive(Clone)]`.
impl<World> Clone for Cucumber<World> {
    fn clone(&self) -> Self {
        match self {
            Self::Started => Self::Started,
            Self::Feature(f, ev) => Self::Feature(Arc::clone(f), ev.clone()),
            Self::Finished => Self::Finished,
        }
    }
}

impl<World> Cucumber<World> {
    /// Constructs an event of a [`Feature`] being started.
    ///
    /// [`Feature`]: gherkin::Feature
    #[must_use]
    pub fn feature_started(feat: Arc<gherkin::Feature>) -> Self {
        Self::Feature(feat, Feature::Started)
    }

    /// Constructs an event of a [`Rule`] being started.
    ///
    /// [`Rule`]: gherkin::Rule
    #[must_use]
    pub fn rule_started(
        feat: Arc<gherkin::Feature>,
        rule: Arc<gherkin::Rule>,
    ) -> Self {
        Self::Feature(feat, Feature::Rule(rule, Rule::Started))
    }

    /// Constructs an event of a [`Feature`] being finished.
    ///
    /// [`Feature`]: gherkin::Feature
    #[must_use]
    pub fn feature_finished(feat: Arc<gherkin::Feature>) -> Self {
        Self::Feature(feat, Feature::Finished)
    }

    /// Constructs an event of a [`Rule`] being finished.
    ///
    /// [`Rule`]: gherkin::Rule
    #[must_use]
    pub fn rule_finished(
        feat: Arc<gherkin::Feature>,
        rule: Arc<gherkin::Rule>,
    ) -> Self {
        Self::Feature(feat, Feature::Rule(rule, Rule::Finished))
    }

    /// Constructs a [`Cucumber`] event from the given [`Scenario`] event.
    #[must_use]
    pub fn scenario(
        feat: Arc<gherkin::Feature>,
        rule: Option<Arc<gherkin::Rule>>,
        scenario: Arc<gherkin::Scenario>,
        event: Scenario<World>,
    ) -> Self {
        Self::Feature(
            feat,
            if let Some(r) = rule {
                Feature::Rule(r, Rule::Scenario(scenario, event))
            } else {
                Feature::Scenario(scenario, event)
            },
        )
    }
}

/// Event specific to a particular [Feature].
///
/// [Feature]: https://cucumber.io/docs/gherkin/reference/#feature
#[derive(Debug)]
pub enum Feature<World> {
    /// [`Feature`] execution being started.
    ///
    /// [`Feature`]: gherkin::Feature
    Started,

    /// [`Rule`] event.
    Rule(Arc<gherkin::Rule>, Rule<World>),

    /// [`Scenario`] event.
    Scenario(Arc<gherkin::Scenario>, Scenario<World>),

    /// [`Feature`] execution being finished.
    ///
    /// [`Feature`]: gherkin::Feature
    Finished,
}

// Manual implementation is required to omit the redundant `World: Clone` trait
// bound imposed by `#[derive(Clone)]`.
impl<World> Clone for Feature<World> {
    fn clone(&self) -> Self {
        match self {
            Self::Started => Self::Started,
            Self::Rule(r, ev) => Self::Rule(Arc::clone(r), ev.clone()),
            Self::Scenario(s, ev) => Self::Scenario(Arc::clone(s), ev.clone()),
            Self::Finished => Self::Finished,
        }
    }
}

/// Event specific to a particular [Rule].
///
/// [Rule]: https://cucumber.io/docs/gherkin/reference/#rule
#[derive(Debug)]
pub enum Rule<World> {
    /// [`Rule`] execution being started.
    ///
    /// [`Rule`]: gherkin::Rule
    Started,

    /// [`Scenario`] event.
    Scenario(Arc<gherkin::Scenario>, Scenario<World>),

    /// [`Rule`] execution being finished.
    ///
    /// [`Rule`]: gherkin::Rule
    Finished,
}

// Manual implementation is required to omit the redundant `World: Clone` trait
// bound imposed by `#[derive(Clone)]`.
impl<World> Clone for Rule<World> {
    fn clone(&self) -> Self {
        match self {
            Self::Started => Self::Started,
            Self::Scenario(s, ev) => Self::Scenario(Arc::clone(s), ev.clone()),
            Self::Finished => Self::Finished,
        }
    }
}

/// Event specific to a particular [Step].
///
/// [Step]: https://cucumber.io/docs/gherkin/reference/#step
#[derive(Debug)]
pub enum Step<World> {
    /// [`Step`] execution being started.
    ///
    /// [`Step`]: gherkin::Step
    Started,

    /// [`Step`] being skipped.
    ///
    /// That means there is no [`Regex`] matching [`Step`] in a
    /// [`step::Collection`].
    ///
    /// [`Regex`]: regex::Regex
    /// [`Step`]: gherkin::Step
    /// [`step::Collection`]: crate::step::Collection
    Skipped,

    /// [`Step`] passed.
    ///
    /// [`Step`]: gherkin::Step
    Passed(regex::CaptureLocations),

    /// [`Step`] failed.
    ///
    /// [`Step`]: gherkin::Step
    Failed(
        Option<regex::CaptureLocations>,
        Option<Arc<World>>,
        StepError,
    ),
}

// Manual implementation is required to omit the redundant `World: Clone` trait
// bound imposed by `#[derive(Clone)]`.
impl<World> Clone for Step<World> {
    fn clone(&self) -> Self {
        match self {
            Self::Started => Self::Started,
            Self::Skipped => Self::Skipped,
            Self::Passed(captures) => Self::Passed(captures.clone()),
            Self::Failed(captures, w, info) => {
                Self::Failed(captures.clone(), w.clone(), info.clone())
            }
        }
    }
}

/// Error of executing a [`Step`].
///
/// [`Step`]: gherkin::Step
#[derive(Clone, Debug, Display, Error, From)]
pub enum StepError {
    /// [`Step`] matches multiple [`Regex`]es.
    ///
    /// [`Regex`]: regex::Regex
    /// [`Step`]: gherkin::Step
    #[display(fmt = "Step match is ambiguous: {}", _0)]
    AmbiguousMatch(step::AmbiguousMatchError),

    /// [`Step`] panicked.
    ///
    /// [`Step`]: gherkin::Step
    #[display(fmt = "Step panicked. Captured output: {}", "coerce_error(_0)")]
    Panic(#[error(not(source))] Info),
}

/// Type of a hook executed before or after all [`Scenario`]'s [`Step`]s.
///
/// [`Scenario`]: gherkin::Scenario
/// [`Step`]: gherkin::Step
#[derive(Clone, Copy, Debug)]
pub enum HookType {
    /// Executing on each [`Scenario`] before running all [`Step`]s.
    ///
    /// [`Scenario`]: gherkin::Scenario
    /// [`Step`]: gherkin::Step
    Before,

    /// Executing on each [`Scenario`] after running all [`Step`]s.
    ///
    /// [`Scenario`]: gherkin::Scenario
    /// [`Step`]: gherkin::Step
    After,
}

impl fmt::Display for HookType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // TODO: Use "{self}" syntax once MSRV bumps above 1.58.
        write!(f, "{:?}", self)
    }
}

/// Event of running [`Before`] or [`After`] hook.
///
/// [`After`]: HookType::After
/// [`Before`]: HookType::Before
#[derive(Debug)]
pub enum Hook<World> {
    /// Hook execution being started.
    Started,

    /// Hook passed.
    Passed,

    /// Hook failed.
    Failed(Option<Arc<World>>, Info),
}

// Manual implementation is required to omit the redundant `World: Clone` trait
// bound imposed by `#[derive(Clone)]`.
impl<World> Clone for Hook<World> {
    fn clone(&self) -> Self {
        match self {
            Self::Started => Self::Started,
            Self::Passed => Self::Passed,
            Self::Failed(w, i) => Self::Failed(w.clone(), Arc::clone(i)),
        }
    }
}

/// Event specific to a particular [Scenario].
///
/// [Scenario]: https://cucumber.io/docs/gherkin/reference/#example
#[derive(Debug)]
pub enum Scenario<World> {
    /// [`Scenario`] execution being started.
    ///
    /// [`Scenario`]: gherkin::Scenario
    Started,

    /// [`Hook`] event.
    Hook(HookType, Hook<World>),

    /// [`Background`] [`Step`] event.
    ///
    /// [`Background`]: gherkin::Background
    Background(Arc<gherkin::Step>, Step<World>),

    /// [`Step`] event.
    Step(Arc<gherkin::Step>, Step<World>),

    /// [`Scenario`] execution being finished.
    ///
    /// [`Scenario`]: gherkin::Scenario
    Finished,
}

// Manual implementation is required to omit the redundant `World: Clone` trait
// bound imposed by `#[derive(Clone)]`.
impl<World> Clone for Scenario<World> {
    fn clone(&self) -> Self {
        match self {
            Self::Started => Self::Started,
            Self::Hook(ty, ev) => Self::Hook(*ty, ev.clone()),
            Self::Background(bg, ev) => {
                Self::Background(Arc::clone(bg), ev.clone())
            }
            Self::Step(st, ev) => Self::Step(Arc::clone(st), ev.clone()),
            Self::Finished => Self::Finished,
        }
    }
}

impl<World> Scenario<World> {
    /// Constructs an event of a [`Scenario`] hook being started.
    ///
    /// [`Scenario`]: gherkin::Scenario
    #[must_use]
    pub const fn hook_started(which: HookType) -> Self {
        Self::Hook(which, Hook::Started)
    }

    /// Constructs an event of a passed [`Scenario`] hook.
    ///
    /// [`Scenario`]: gherkin::Scenario
    #[must_use]
    pub const fn hook_passed(which: HookType) -> Self {
        Self::Hook(which, Hook::Passed)
    }

    /// Constructs an event of a failed [`Scenario`] hook.
    ///
    /// [`Scenario`]: gherkin::Scenario
    #[must_use]
    pub fn hook_failed(
        which: HookType,
        world: Option<Arc<World>>,
        info: Info,
    ) -> Self {
        Self::Hook(which, Hook::Failed(world, info))
    }

    /// Constructs an event of a [`Step`] being started.
    ///
    /// [`Step`]: gherkin::Step
    #[must_use]
    pub fn step_started(step: Arc<gherkin::Step>) -> Self {
        Self::Step(step, Step::Started)
    }

    /// Constructs an event of a [`Background`] [`Step`] being started.
    ///
    /// [`Background`]: gherkin::Background
    /// [`Step`]: gherkin::Step
    #[must_use]
    pub fn background_step_started(step: Arc<gherkin::Step>) -> Self {
        Self::Background(step, Step::Started)
    }

    /// Constructs an event of a passed [`Step`].
    ///
    /// [`Step`]: gherkin::Step
    #[must_use]
    pub fn step_passed(
        step: Arc<gherkin::Step>,
        captures: regex::CaptureLocations,
    ) -> Self {
        Self::Step(step, Step::Passed(captures))
    }

    /// Constructs an event of a passed [`Background`] [`Step`].
    ///
    /// [`Background`]: gherkin::Background
    /// [`Step`]: gherkin::Step
    #[must_use]
    pub fn background_step_passed(
        step: Arc<gherkin::Step>,
        captures: regex::CaptureLocations,
    ) -> Self {
        Self::Background(step, Step::Passed(captures))
    }

    /// Constructs an event of a skipped [`Step`].
    ///
    /// [`Step`]: gherkin::Step
    #[must_use]
    pub fn step_skipped(step: Arc<gherkin::Step>) -> Self {
        Self::Step(step, Step::Skipped)
    }
    /// Constructs an event of a skipped [`Background`] [`Step`].
    ///
    /// [`Background`]: gherkin::Background
    /// [`Step`]: gherkin::Step
    #[must_use]
    pub fn background_step_skipped(step: Arc<gherkin::Step>) -> Self {
        Self::Background(step, Step::Skipped)
    }

    /// Constructs an event of a failed [`Step`].
    ///
    /// [`Step`]: gherkin::Step
    #[must_use]
    pub fn step_failed(
        step: Arc<gherkin::Step>,
        captures: Option<regex::CaptureLocations>,
        world: Option<Arc<World>>,
        info: impl Into<StepError>,
    ) -> Self {
        Self::Step(step, Step::Failed(captures, world, info.into()))
    }

    /// Constructs an event of a failed [`Background`] [`Step`].
    ///
    /// [`Background`]: gherkin::Background
    /// [`Step`]: gherkin::Step
    #[must_use]
    pub fn background_step_failed(
        step: Arc<gherkin::Step>,
        captures: Option<regex::CaptureLocations>,
        world: Option<Arc<World>>,
        info: impl Into<StepError>,
    ) -> Self {
        Self::Background(step, Step::Failed(captures, world, info.into()))
    }
}
