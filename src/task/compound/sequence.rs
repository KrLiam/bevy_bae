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
    mut task_relations: Local<QueryState<&Tasks>>,
    mut individual_tasks: Local<
        QueryState<
            (
                Entity,
                Has<Operator>,
                Option<&TypeErasedCompoundTask>,
                Option<&Conditions>,
                Option<&Effects>,
            ),
            Or<(With<Operator>, With<TypeErasedCompoundTask>)>,
        >,
    >,
    mut conditions: Local<QueryState<&Condition>>,
    mut effects: Local<QueryState<&Effect>>,
    mut individual_tasks_scratch: Local<
        Vec<(
            Entity,
            bool,
            Option<TypeErasedCompoundTask>,
            Option<Conditions>,
            Option<Effects>,
        )>,
    >,
) -> DecomposeResult {
    let Ok(tasks) = task_relations.get(world, ctx.compound_task) else {
        return DecomposeResult::Failure;
    };
    individual_tasks_scratch.extend(individual_tasks.iter_many(world, tasks).map(
        |(task_entity, has_operator, compound_task, condition_relations, effect_relations)| {
            (
                task_entity,
                has_operator,
                compound_task.cloned(),
                condition_relations.cloned(),
                effect_relations.cloned(),
            )
        },
    ));
    let mut found_anything = false;
    for (task_entity, has_operator, compound_task, condition_relations, effect_relations) in
        individual_tasks_scratch.drain(..)
    {
        if let Some(condition_relations) = condition_relations {
            for condition in conditions.iter_many(world, condition_relations.iter()) {
                if !condition.is_fullfilled(&mut ctx.world_state) {
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
                    world_state: ctx.world_state.clone(),
                    plan: ctx.plan.clone(),
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
        if let Some(effect_relations) = effect_relations {
            for effect in effects.iter_many(world, effect_relations.iter()) {
                effect.apply(&mut ctx.world_state);
            }
            ctx.plan.steps.push(PlanStep::ApplyEffects(task_entity));
        }
        found_anything = true;
    }

    if found_anything {
        DecomposeResult::Success {
            plan: ctx.plan,
            world_state: ctx.world_state,
        }
    } else {
        DecomposeResult::Failure
    }
}
