//! Contains the [`Sequence`] [`CompoundTask`]

use std::ops::DerefMut;

use crate::{
    plan::PlanStep, prelude::*, task::compound::{Decompose, DecomposeId, DecomposeInput, DecomposeResult, TaskTuple},
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
    In(input): In<DecomposeInput>,
    world: &mut World,
    mut d: Local<Decompose>,
    mut tasks_buffer: Local<Vec<TaskTuple>>,
) -> DecomposeResult {
    let d = d.deref_mut();
    let Ok(tasks) = d.q_task_lists.get(world, input.compound_task) else {
        return DecomposeResult::Failure;
    };
    if tasks.is_empty() { return DecomposeResult::Failure }

    tasks_buffer.extend(d.q_tasks.iter_many(world, tasks).map(
        |(task_entity, has_operator, compound_task)| {
            (task_entity, has_operator, compound_task.cloned())
        },
    ));

    for (task_entity, has_operator, compound_task) in tasks_buffer.drain(..)
    {
        if let Some(valid) = d.validate_conditions(world, task_entity, input.ctx_mut())
            && !valid
        {
            return DecomposeResult::Failure;
        }
        
        if has_operator {
            input.ctx_mut().plan.steps.push(PlanStep::RunOperator(task_entity));
        } else if let Some(compound_task) = compound_task {
            let result = world.run_system_with(
                compound_task.decompose,
                input.with_task(task_entity),
            );
            world.flush();
            match result {
                Ok(DecomposeResult::Success) => {}
                Ok(DecomposeResult::Rejection) => return DecomposeResult::Rejection,
                Ok(DecomposeResult::Failure) | Err(_) => return DecomposeResult::Failure,
            }
        } else {
            unreachable!()
        }
        if input.ctx_mut().plan.is_empty() {
            return DecomposeResult::Failure;
        }
        d.apply_effects(world, task_entity, input.ctx_mut());
    }
    DecomposeResult::Success
}
