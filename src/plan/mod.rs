//! Contains the [`Plan`] component and types for operating on it.

use bevy_ecs::{entity_disabling::Disabled, lifecycle::HookContext, query::QueryEntityError, world::DeferredWorld};

use crate::{plan::mtr::Mtr, prelude::*, task::OperatorId};

pub(crate) mod execution;
pub mod mtr;
pub mod update;

/// Specifies the domain used by a planner entity.
#[derive(Component, Default)]
#[component(on_insert = Self::on_insert_hook)]
pub enum PlanDomain {
    /// The planner entity defines its own task hierarchy.
    #[default]
    This,
    /// The planner entity reuses a task hierarchy defined by another entity.
    Entity(Entity),
}

impl PlanDomain {
    fn on_insert_hook(mut world: DeferredWorld, context: HookContext) {
        let mut cmds = world.commands();
        cmds.entity(context.entity).insert_if_new(Plan::default());
    }
}

/// A full plan of operators to execute. If this is empty, either through manually clearing it, inserting it, when it runs out of operators, or fails to execute them,
/// the plan will be recomputed in the next fixed frame.
#[derive(Component, Clone, Default, PartialEq, Eq, Reflect, Debug)]
#[reflect(Component)]
#[require(Props, PlanDomain, PlanScope)]
pub struct Plan {
    /// The planned steps.
    pub steps: Vec<PlanStep>,
    /// The index of the current step.
    pub index: usize,
    /// Whether the plan execution is paused.
    pub paused: bool,
    /// Whether this plan can be replaced. If set to `true`
    /// all `UpdatePlan` events are postponed until the plan
    /// is unlocked.
    pub locked: bool,
    /// The [`OperatorStatus`] returned by the current operator.
    pub status: Option<OperatorStatus>,
    /// The [`Mtr`] of the full plan when it was created.
    pub mtr: Mtr,

    /// If `locked` is `true` and a `UpdatePlan` event was triggered,
    /// this flag is set to `true`.
    attempted_replan: bool,
}

impl Plan {
    /// Creates a new empty plan. Inserting such a plan will immediately trigger a replan at the next fixed frame.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the current plan with a new empty plan. Doing so will immediately trigger a replan at the next fixed frame.
    pub fn clear(&mut self) {
        self.steps.clear();
        self.index = 0;
        self.status = None;
        self.mtr.clear();
    }

    /// Sets the attributes of this plan based on another.
    pub fn set_from(&mut self, other: &Plan) {
        self.clear();
        self.steps.extend(other.steps.iter().cloned());
        self.index = other.index;
        self.status = other.status;
        self.mtr.extend(other.mtr.iter().cloned());
    }

    /// Gets the current step.
    pub fn current_step(&self) -> Option<PlanStep> {
        self.steps.get(self.index).cloned()
    }

    /// Returns whether this plan is over.
    pub fn is_empty(&self) -> bool {
        self.index >= self.steps.len()
    }

    /// Return all operator entities.
    pub fn operators_total(&self) -> impl Iterator<Item=Entity> {
        self.steps.iter()
            .filter_map(|step| match step {
                PlanStep::RunOperator(entity) => Some(*entity),
                _ => None
            })
    }

    /// Return all effects.
    pub fn effects_total(&self) -> impl Iterator<Item=Entity> {
        self.steps.iter()
            .filter_map(|step| match step {
                PlanStep::ApplyEffects(entity) => Some(*entity),
                _ => None
            })
    }

    /// Return the steps left.
    pub fn steps_left(&self) -> impl Iterator<Item=&PlanStep> {
        self.steps.iter().skip(self.index)
    }

    /// Return the operators left.
    pub fn operators_left(&self) -> impl Iterator<Item=Entity> {
        self.operators_total().skip(self.index)
    }

    /// Return the effects to be applied left.
    pub fn effects_left(&self) -> impl Iterator<Item=Entity> {
        self.effects_total().skip(self.index)
    }

    /// Advances plan execution to the next step.
    pub fn advance(&mut self) {
        self.index = usize::min(self.index + 1, self.steps.len());
    }
}


#[derive(Component, Debug, Default)]
pub(crate) struct PlanScope {
    pub stack: Vec<Entity>,
}
impl PlanScope {
    #[inline]
    pub fn push(&mut self, task: Entity) {
        self.stack.push(task);
    }

    #[inline]
    pub fn pop(&mut self, task: Entity) -> bool {
        let matched = self.stack.last() == Some(&task);
        if matched {
            self.stack.pop();
        }
        matched
    }
}

/// A step in the plan execution.
#[derive(Debug, Clone, PartialEq, Eq, Reflect)]
pub enum PlanStep {
    /// Checks if all conditions specified by [`Entity`]
    /// are fullfilled. The entity must have [`Conditions`].
    ValidateConditions(Entity),
    /// Runs the operator system of [`Entity`] with the [`Operator`] component.
    RunOperator(Entity),
    /// Runs the enter operator system of [`Entity`].
    RunEnterOperator {
        /// The task entity.
        entity: Entity,
        /// Whether should push this task onto [`PlanStack`].
        push_stack: bool,
    },
    /// Runs the exit operator system of [`Entity`].
    RunExitOperator(Entity),
    /// Applies the effects of [`Entity`] with the [`Effects`] component;
    ApplyEffects(Entity),
    /// Runs a system.
    RunSystem {
        /// The task that originated this step.
        entity: Entity,
        /// The system to be executed.
        #[reflect(ignore)]
        system: Option<OperatorId>,
        /// Whether execution immediately continues
        /// after processing this step.
        instant: bool,
    },
    /// Jumps execution to step at `index`.
    Jump {
        /// The task that originated this step.
        entity: Entity,
        /// The destination index.
        index: usize,
    },
}
impl PlanStep {
    /// Returns the entity referenced by this step.
    pub fn entity(&self) -> Entity {
        match self {
            PlanStep::ValidateConditions(entity) => *entity,
            PlanStep::RunOperator(entity) => *entity,
            PlanStep::RunEnterOperator{ entity, .. } => *entity,
            PlanStep::RunExitOperator(entity) => *entity,
            PlanStep::ApplyEffects(entity) => *entity,
            PlanStep::RunSystem { entity, .. } => *entity,
            PlanStep::Jump { entity, .. } => *entity,
        }
    }
}



/// Describe a step to be performed while checking if a plan is still valid.
#[derive(Debug, Clone)]
pub enum CheckStep {
    /// Checks if the condition still produces the same result.
    Condition {
        /// The condition to be checked.
        condition: Condition,
        /// The expected value.
        expected: bool,
    },
    /// Applies the effects specified by the entity.
    Effects {
        /// The entity.
        entity: Entity,
    }
}

/// Enables automatic replanning when properties that invalidate
/// the current plan are modified.
#[derive(Component, Debug, Default)]
pub struct PlanReactivity {
    /// The steps that must be performed during validation.
    check_steps: Vec<CheckStep>,
}

/// An [`EntityEvent`] for logging a given plan via [`info!`]
#[derive(EntityEvent, Debug)]
pub struct LogPlan {
    entity: Entity,
}

impl LogPlan {
    /// Creates a new [`LogPlan`] event for the given entity.
    pub fn new(entity: Entity) -> Self {
        LogPlan { entity }
    }
}

impl From<Entity> for LogPlan {
    fn from(entity: Entity) -> Self {
        LogPlan::new(entity)
    }
}

pub(crate) fn log_plan(
    log: On<LogPlan>,
    plans: Query<&Plan, Allow<Disabled>>,
    names: Query<NameOrEntity, Allow<Disabled>>,
) -> Result {
    let plan_entity = log.entity;
    let plan = plans.get(plan_entity)?;
    let name = |entity| -> Result<String, QueryEntityError> {
        names.get(entity).map(|n| n.entity_and_name())
    };
    let plan_name = name(plan_entity)?;
    let mut log = String::new();
    log.push_str(&format!("plan {plan_name}:\n"));
    log.push_str(&format!("- mtr: {}\n", plan.mtr));
    log.push_str(&format!(
        "- total steps ({})\n",
        plan.steps.len()
    ));
    log.push_str(&format!(
        "- steps left ({}):\n",
        plan.steps_left().count()
    ));
    for step in plan.steps_left() {
        match step {
            PlanStep::ValidateConditions(entity) => {
                let name = name(*entity)?;
                log.push_str(&format!("  - conditions: {name}\n"));
            },
            PlanStep::RunOperator(entity) => {
                let name = name(*entity)?;
                log.push_str(&format!("  - operator: {name}\n"));
            },
            PlanStep::RunSystem { entity,.. } => {
                let name = name(*entity)?;
                log.push_str(&format!("  - system: {name}\n"));
            },
            PlanStep::RunEnterOperator { entity, .. } => {
                let name = name(*entity)?;
                log.push_str(&format!("  - enter operator: {name}\n"));
            },
            PlanStep::RunExitOperator(entity) => {
                let name = name(*entity)?;
                log.push_str(&format!("  - exit operator: {name}\n"));
            },
            PlanStep::ApplyEffects(entity) => {
                let name = name(*entity)?;
                log.push_str(&format!("  - effects: {name}\n"));
            },
            PlanStep::Jump { entity, .. } => {
                let name = name(*entity)?;
                log.push_str(&format!("  - jump: {name}\n"));
            },
        }
    }
    info!("{}", log.trim());
    Ok(())
}
