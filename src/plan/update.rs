//! Contains the [`UpdatePlan`] [`EntityEvent`].

use bevy_ecs::error::{DefaultErrorHandler, HandleError as _};
use bevy_ecs::system::command::run_system_cached_with;
use core::marker::PhantomData;

use crate::plan::{PlanDomain, PlanReactivity, PlanStep};
use crate::plan::mtr::Mtr;
use crate::prelude::*;
use crate::task::compound::{Decompose, DecomposeContext, DecomposeInput, DecomposeResult};
use crate::task::observer::ObserverOperatorSystems;

/// [`EntityEvent`] for updating a plan. Trigger this on an entity with a [`Plan`] to update its plan.
/// Updating it will only have an effect if the new plan found has a higher priority than the current one.
/// This ensures that ongoing [`Sequence`]s are not suddenly interrupted when updating the plan.
/// If you want to instead wipe the slate clean, insert [`Plan::new`] instead, or call [`Plan::clear`].
#[derive(EntityEvent)]
pub struct UpdatePlan {
    /// The entity holding the [`Plan`] to update.
    #[event_target]
    pub entity: Entity,
}

impl From<Entity> for UpdatePlan {
    fn from(entity: Entity) -> Self {
        Self { entity }
    }
}

impl UpdatePlan {
    /// Create a new [`UpdatePlan`] event for the given entity. Usually called with the [`EntityCommands::trigger`] API.
    pub fn new(entity: Entity) -> Self {
        Self::from(entity)
    }
}

/// Event triggered automatically when a plan is replaced.
#[derive(EntityEvent)]
pub struct ReplacePlan {
    /// The entity holding the [`Plan`] that was replaced.
    #[event_target]
    pub entity: Entity,
    /// The previous value of the [`Plan`]. To read the current value, query it in your observer.
    pub old: Plan,
    /// Here so users cannot accidentally create a [`ReplacePlan`] when they were
    /// actually looking for [`UpdatePlan`].
    _pd: PhantomData<()>,
}

/// Observer that runs the logic for updating plans.
pub fn update_plan(
    update: On<UpdatePlan>,
    mut commands: Commands,
    error_handler: Option<Res<DefaultErrorHandler>>,
) {
    let entity = update.entity;
    let error_handler = error_handler.map(|h| *h).unwrap_or_default();
    commands.queue(
        run_system_cached_with(update_plan_inner, UpdatePlan { entity })
            .handle_error_with(error_handler.0),
    );
}

/// 
pub fn update_plan_inner(
    update: In<UpdatePlan>,
    world: &mut World,
    mut plans: Local<QueryState<&PlanDomain>>,
    mut reacts: Local<QueryState<&mut PlanReactivity>>,
    mut d: Local<Decompose>,
) -> Result {
    let executor = update.entity;
    let Ok(domain) = plans.get(world, executor)
    else { return Err(BevyError::from("Called `update_plan` on entity without Plan.")) };
    let root = match domain {
        PlanDomain::This => executor,
        PlanDomain::Entity(entity) => *entity,
    };

    let mut ctx = DecomposeContext::default();
    ctx.world_state.extend(world.entity(update.entity).props());

    let Some((entity, has_enter, has_exit, has_operator, has_observer, compound_task))
        = d.get_task(world, root)
    else {
        world.entity_mut(root).insert(Plan::default());
        return Err(BevyError::from("Called `update_plan` for an entity without any tasks. Ensure it has either an `Operator` or a `CompoundTask` like `Select` or `Sequence`".to_string()));
    };

    if has_observer {
        let systems = world.resource::<ObserverOperatorSystems>();
        ctx.plan.steps.push(PlanStep::RunSystem { entity, system: Some(systems.enter), instant: true });
    }

    if has_enter {
        ctx.plan.steps.push(PlanStep::RunEnterOperator { entity, push_stack: has_exit });
    }

    if let Some(false) = d.validate_conditions(world, entity, &mut ctx) {
        ctx.plan.clear();
    }
    else if has_operator {
        // well that was easy: this root has just a single operator
        ctx.plan.steps.push(PlanStep::RunOperator(entity));
    }
    else if let Some(compound_task) = compound_task {
        ctx.previous_mtr = if let Some(plan) = world.entity(root).get::<Plan>() {
            plan.mtr.clone()
        } else {
            Mtr::none()
        };
        
        let input = DecomposeInput {
            planner: root,
            compound_task: root,
            ctx: &mut ctx as *mut _,
        };
        
        let result = world.run_system_with(compound_task.decompose, input)?;
        world.flush();

        match result {
            DecomposeResult::Success => {}
            DecomposeResult::Failure => {
                ctx.plan.clear();
            },
            DecomposeResult::Rejection => return Ok(()),
        }
    };

    if has_observer {
        let systems = world.resource::<ObserverOperatorSystems>();
        ctx.plan.steps.push(PlanStep::RunSystem { entity, system: Some(systems.exit), instant: true });
    }

    d.apply_effects(world, entity, &mut ctx);

    if has_exit {
        ctx.plan.steps.push(PlanStep::RunExitOperator(root));
    }

    if ctx.previous_mtr == ctx.plan.mtr
        && world.entity(root).get::<Plan>().is_some_and(|prev_plan| {
            prev_plan.steps == ctx.plan.steps
        })
    {
        // We found the same plan we are already running. Just keep that one.
        return Ok(());
    }

    if let Ok(mut react) = reacts.get_mut(world, executor) {
        react.check_steps.clear();
        react.check_steps.extend(ctx.checked_steps);
        debug!("reactive plan set checked steps: {:?}",react.check_steps);
    }

    let old_plan = world
        .entity(executor)
        .get::<Plan>()
        .cloned()
        .unwrap_or_default();
    world.entity_mut(executor).insert(ctx.plan);
    world.trigger(ReplacePlan {
        entity: executor,
        old: old_plan,
        _pd: PhantomData,
    });
    Ok(())
}
