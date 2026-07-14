//! Contains the [`Select`] [`CompoundTask`]

use crate::{
    plan::PlanStep, prelude::*, task::compound::{DecomposeId, DecomposeInput, DecomposeResult, TypeErasedCompoundTask},
};

/// A [`CompoundTask`] that decomposes into the first valid subtask.
#[derive(Debug, Component, Default, Reflect)]
#[reflect(Component)]
pub struct Select;

impl CompoundTask for Select {
    fn register_decompose(commands: &mut Commands) -> DecomposeId {
        commands.register_system(decompose_select)
    }
}

fn decompose_select(
    In(mut ctx): In<DecomposeInput>,
    world: &mut World,
    mut q_task_lists: Local<QueryState<&Tasks>>,
    mut q_tasks: Local<
        QueryState<
            (
                Entity,
                Has<Operator>,
                Option<&TypeErasedCompoundTask>,
            ),
            Or<(With<Operator>, With<TypeErasedCompoundTask>)>,
        >,
    >,
    mut q_condition_lists: Local<QueryState<&Conditions>>,
    mut q_conditions: Local<QueryState<(Entity, &Condition)>>,
    mut q_effect_lists: Local<QueryState<&Effects>>,
    mut q_effects: Local<QueryState<(Entity, &Effect)>>,
    mut tasks_buffer: Local<
        Vec<(Entity, bool, Option<TypeErasedCompoundTask>)>,
    >,
) -> DecomposeResult {
    let Ok(tasks) = q_task_lists.get(world, ctx.compound_task) else {
        return DecomposeResult::Failure;
    };
    tasks_buffer.extend(q_tasks.iter_many(world, tasks).map(
        |(task_entity, has_operator, compound_task)| {
            (task_entity, has_operator, compound_task.cloned())
        },
    ));

    'task: for (
        i,
        (task_entity, has_operator, compound_task),
    ) in tasks_buffer.drain(..).enumerate()
    {
        let mtr = ctx.plan.mtr.clone().with(i as u16);
        if mtr > ctx.previous_mtr {
            return DecomposeResult::Rejection;
        }
        if let Ok(condition_relations) = q_condition_lists.get(world, task_entity) {
            for (_, condition) in q_conditions.iter_many(world, condition_relations.iter()) {
                if !condition.is_fullfilled(&mut ctx.world_state) {
                    continue 'task;
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
                Ok(DecomposeResult::Failure) | Err(_) => continue,
            }
        } else {
            unreachable!()
        }
        if ctx.plan.is_empty() {
            return DecomposeResult::Failure;
        }
        if let Ok(effect_relations) = q_effect_lists.get(world, task_entity) {
            for (_, effect) in q_effects.iter_many(world, effect_relations.iter()) {
                effect.apply(&mut ctx.world_state);
            }

            ctx.plan.steps.push(PlanStep::ApplyEffects(task_entity));
        }
        // only use the first match
        ctx.plan.mtr.push(i as u16);
        return DecomposeResult::Success {
            plan: ctx.plan,
            world_state: ctx.world_state,
        };
    }
    DecomposeResult::Failure
}
