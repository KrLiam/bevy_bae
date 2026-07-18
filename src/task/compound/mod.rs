//! Contains types representing a tree of more compound tasks, where the leaves are [`Operator`]s

use bevy_ecs::system::SystemId;

use crate::{
    plan::{Plan, mtr::Mtr},
    prelude::*,
};

pub mod relationship;
pub mod select;
pub mod sequence;

/// Trait implemented for compound tasks. The builtin [`CompoundTask`]s are [`Sequence`] and [`Select`].
/// If you implement this trait, you must also call [`CompoundAppExt::add_compound_task`] to initialize it.
pub trait CompoundTask: Component {
    /// Registers the decomposition system for this compound task.
    fn register_decompose(commands: &mut Commands) -> DecomposeId;
}

/// Type alias for the exact [`SystemId`] used by the [`CompoundTask`] for task decomposition.
pub type DecomposeId = SystemId<In<DecomposeInput>, DecomposeResult>;

/// Input given to a [`CompoundTask`] for task decomposition.
#[derive(Debug)]
pub struct DecomposeInput {
    /// The root entity that is holding the [`Plan`].
    pub planner: Entity,
    /// The entity that is holding the current [`CompoundTask`] we are decomposing.
    pub compound_task: Entity,
    /// The current [`Props`] associated with this step of the decomposition.
    /// Expected to be mutated during decomposition and returned in [`DecomposeResult`].
    pub world_state: Props,
    /// The current [`Plan`] associated with this step of the decomposition.
    /// Expected to be mutated during decomposition and returned in [`DecomposeResult`].
    pub plan: Plan,
    /// The [`Mtr`] of the previous plan.
    /// Used to determine whether the current decomposition should return [`DecomposeResult::Rejection`] because it has a lower priority than the running task.
    pub previous_mtr: Mtr,
}

///
#[derive(Component, Clone)]
pub struct TypeErasedCompoundTask {
    pub(crate) decompose: DecomposeId,
}

impl TypeErasedCompoundTask {
    #[must_use]
    fn new(id: DecomposeId) -> Self {
        Self { decompose: id }
    }
}

/// The result of a decomposition attempt of a [`CompoundTask`].
pub enum DecomposeResult {
    /// The decomposition was successful.
    Success {
        /// A modified copy of [`DecomposeInput::plan`], appended with the decomposition.
        plan: Plan,
        /// A modified copy of [`DecomposeInput::world_state`], updated with the decomposition.
        world_state: Props,
    },
    /// The decomposition would have resulted in a lower priority than the running task.
    Rejection,
    /// The decomposition failed unexpectedly, e.g. something in the ECS is returning unexpected results or being filtered out.
    /// Will trigger a replan.
    Failure,
}

/// Used to allow calling [`CompoundAppExt::add_compound_task`] on [`App`].
pub trait CompoundAppExt {
    /// Registers a new [`CompoundTask`] with the [`App`].
    fn add_compound_task<C: CompoundTask>(&mut self) -> &mut Self;
}

impl CompoundAppExt for App {
    fn add_compound_task<C: CompoundTask>(&mut self) -> &mut Self {
        self.add_observer(insert_type_erased_task::<C>)
            .add_observer(remove_type_erased_task::<C>);
        self
    }
}

fn insert_type_erased_task<C: CompoundTask>(insert: On<Insert, C>, mut commands: Commands) {
    let system_id = C::register_decompose(&mut commands);
    commands
        .entity(insert.entity)
        .try_insert(TypeErasedCompoundTask::new(system_id));
}
fn remove_type_erased_task<C: CompoundTask>(remove: On<Remove, C>, mut commands: Commands) {
    commands
        .entity(remove.entity)
        .try_remove::<TypeErasedCompoundTask>();
}
