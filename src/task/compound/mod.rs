//! Contains types representing a tree of more compound tasks, where the leaves are [`Operator`]s

use bevy_ecs::system::SystemId;

use crate::{
    plan::{CheckStep, Plan, PlanStep, mtr::Mtr}, prelude::*, task::scope::{EnterOperator, ExitOperator},
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

/// Context during task decomposition.
#[derive(Debug, Clone)]
pub struct DecomposeContext {
    /// The current [`Props`] associated with this step of the decomposition.
    pub world_state: Props,
    /// The current [`Plan`] associated with this step of the decomposition.
    pub plan: Plan,
    /// The [`Mtr`] of the previous plan.
    pub previous_mtr: Mtr,
    /// The conditions checked during the planning.
    pub checked_steps: Vec<CheckStep>,
}
impl Default for DecomposeContext {
    fn default() -> Self {
        Self {
            world_state: Props::default(),
            plan: Plan::default(),
            previous_mtr: Mtr::none(),
            checked_steps: Vec::with_capacity(16),
        }
    }
}
impl DecomposeContext {
    /// Clears the decomposition context.
    pub fn clear(&mut self) {
        self.world_state.clear();
        self.plan.clear();
        self.previous_mtr.clear();
    }

    /// Sets the attributes of this context based on another.
    pub fn set_from(&mut self, other: &DecomposeContext) {
        self.clear();
        self.plan.set_from(&other.plan);
        self.world_state.extend(&other.world_state);
        self.previous_mtr.extend(other.previous_mtr.iter().cloned());
    }
}

/// Input given to a [`CompoundTask`] for task decomposition.
#[derive(Debug, Clone)]
pub struct DecomposeInput {
    /// The root entity that is holding the [`Plan`].
    pub planner: Entity,
    /// The entity that is holding the current [`CompoundTask`] we are decomposing.
    pub compound_task: Entity,
    /// A pointer to the `DecomposeContext`.
    pub ctx: *mut DecomposeContext,
}

// Ensure DecomposeInput is Send and Sync so it can be passed to systems.
unsafe impl Send for DecomposeInput {}
unsafe impl Sync for DecomposeInput {}

impl DecomposeInput {
    /// Returns a new `DecomposeInput` with the same `planner` and `ctx`, but a different `compound_task`.
    #[inline(always)]
    pub fn with_task(&self, task_entity: Entity) -> Self {
        Self {
            planner: self.planner,
            compound_task: task_entity,
            ctx: self.ctx,
        }
    }

    /// Returns a mutable reference to the underlying `DecomposeContext`.
    /// 
    /// # Safety
    /// 
    /// This is safe because `DecomposeInput` is only constructed synchronously in `update_plan_inner`
    /// with a pointer to a local `DecomposeContext` that lives on the stack.
    /// The pointer is guaranteed to be valid for the entire duration of the decomposition tree evaluation.
    /// We do not create multiple overlapping mutable references because the decompose systems process
    /// sequentially, dropping the mutable reference before moving on.
    #[inline(always)]
    #[allow(clippy::mut_from_ref)]
    pub fn ctx_mut(&self) -> &mut DecomposeContext {
        unsafe { &mut *self.ctx }
    }
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
    Success,
    /// The decomposition would have resulted in a lower priority than the running task.
    Rejection,
    /// The decomposition failed unexpectedly, e.g. something in the ECS is returning unexpected results or being filtered out.
    /// Will trigger a replan.
    Failure,
}

/// Task data queried by `Decompose`.
pub type TaskTuple = (Entity, bool, bool, bool, Option<TypeErasedCompoundTask>);

/// Helper for decomposition.
#[allow(missing_docs)]
#[derive(FromWorld)]
pub struct Decompose {
    pub q_task_lists: QueryState<&'static Tasks>,
    pub q_tasks: QueryState<
        (
            Entity,
            Has<EnterOperator>,
            Has<ExitOperator>,
            Has<Operator>,
            Option<&'static TypeErasedCompoundTask>,
        ),
        Or<(With<Operator>, With<TypeErasedCompoundTask>)>,
    >,
    pub q_condition_lists: QueryState<&'static Conditions>,
    pub q_conditions: QueryState<&'static Condition>,
    pub q_effect_lists: QueryState<&'static Effects>,
    pub q_effects: QueryState<&'static Effect>,
    pub tasks_buffer: Vec<TaskTuple>,
}

impl Decompose {
    /// Validates the given conditions and records them in the `DecomposeContext`.
    #[inline(always)]
    pub fn validate_conditions(
        &mut self,
        world: &World,
        entity: Entity,
        ctx: &mut DecomposeContext,
    ) -> Option<bool> {
        let conditions = self.q_condition_lists.get(world, entity).ok()?;

        let mut all_fulfilled = true;

        for condition in self.q_conditions.iter_many(world, conditions.iter()) {
            let result = condition.is_fullfilled(&ctx.world_state);
            ctx.checked_steps.push(
                CheckStep::Condition { condition: condition.clone(), expected: result }
            );
            if !result {
                all_fulfilled = false;
                break;
            }
        }

        if all_fulfilled {
            ctx.plan.steps.push(PlanStep::ValidateConditions(entity));
        }

        Some(all_fulfilled)
    }

    /// Applies the effects specified by the `Effects` component of
    /// the entity onto the context.
    #[inline(always)]
    pub fn apply_effects(
        &mut self,
        world: &World,
        entity: Entity,
        ctx: &mut DecomposeContext,
    ) -> Option<()> {
        let effects = self.q_effect_lists.get(world, entity).ok()?;
        for entity in effects {
            if let Ok(effect) = self.q_effects.get(world, entity) {
                let effect = effect.clone();
                effect.apply(&mut ctx.world_state);
            }
        }

        ctx.plan.steps.push(PlanStep::ApplyEffects(entity));
        ctx.checked_steps.push(CheckStep::Effects { entity });

        Some(())
    }
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
