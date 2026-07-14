//! Contains the [`Sequence`] [`CompoundTask`]

use crate::{
    plan::PlanStep, prelude::*, task::compound::{DecomposeId, DecomposeInput, DecomposeResult, TypeErasedCompoundTask},
};

/// A [`CompoundTask`] that decomposes into all subtasks, given that they are all valid.
#[derive(Debug, Component, Default, Reflect)]
#[reflect(Component)]
pub struct Sequence;

impl CompoundTask for Sequence {
    fn register_decompose(commands: &mut Commands) -> DecomposeId {
        commands.register_system(decompose_sequence)
    }
}

fn decompose_sequence(
    In(mut ctx): In<DecomposeInput>,
    world: &mut World,
    mut q_task_lists: Local<QueryState<&Tasks>>,
    mut q_tasks: Local<
        QueryState<
            (
                Entity,
                Has<Operator>,
                Option<&TypeErasedCompoundTask>,
                Has<Conditions>,
                Has<Effects>,
            ),
            Or<(With<Operator>, With<TypeErasedCompoundTask>)>,
        >,
    >,
    mut q_condition_lists: Local<QueryState<&Conditions>>,
    mut q_conditions: Local<QueryState<&Condition>>,
    mut q_effect_lists: Local<QueryState<&Effects>>,
    mut q_effects: Local<QueryState<&Effect>>,
    mut tasks_buffer: Local<Vec<(Entity, bool, Option<TypeErasedCompoundTask>, bool, bool)>>,
) -> DecomposeResult {
    let Ok(tasks) = q_task_lists.get(world, ctx.compound_task) else {
        return DecomposeResult::Failure;
    };
    if tasks.is_empty() { return DecomposeResult::Failure }

    tasks_buffer.extend(q_tasks.iter_many(world, tasks).map(
        |(task_entity, has_operator, compound_task, has_conditions, has_effects)| {
            (task_entity, has_operator, compound_task.cloned(), has_conditions, has_effects)
        },
    ));

    for (task_entity, has_operator, compound_task, has_conditions, has_effects) in
        tasks_buffer.drain(..)
    {
        if has_conditions &&
            let Ok(conditions_relation) = q_condition_lists.get(world, task_entity)
        {
            for condition in q_conditions.iter_many(world, conditions_relation.iter()) {
                if !condition.is_fullfilled(&ctx.world_state) {
                    return DecomposeResult::Failure;
                }
            }

            ctx.plan.steps.push(PlanStep::ValidateConditions(task_entity));
        }
        
        if has_operator {
            ctx.plan.steps.push(PlanStep::RunOperator(task_entity));
        } else if let Some(compound_task) = compound_task {
            let result = world.run_system_with(
                compound_task.decompose,
                DecomposeInput {
                    planner: ctx.planner,
                    compound_task: task_entity,
                    world_state: ctx.world_state,
                    plan: ctx.plan,
                    previous_mtr: ctx.previous_mtr.clone(),
                },
            );
            world.flush();
            match result {
                Ok(DecomposeResult::Success { plan, world_state }) => {
                    ctx.plan = plan;
                    ctx.world_state = world_state;
                }
                Ok(DecomposeResult::Rejection) => return DecomposeResult::Rejection,
                Ok(DecomposeResult::Failure) | Err(_) => return DecomposeResult::Failure,
            }
        } else {
            unreachable!()
        }
        if ctx.plan.is_empty() {
            return DecomposeResult::Failure;
        }
        if has_effects &&
            let Ok(effects_relation) = q_effect_lists.get(world, task_entity) {
            for effect in q_effects.iter_many(world, effects_relation.iter()) {
                effect.apply(&mut ctx.world_state);
            }
            ctx.plan.steps.push(PlanStep::ApplyEffects(task_entity));
        }
    }

    DecomposeResult::Success {
        plan: ctx.plan,
        world_state: ctx.world_state,
    }
}
