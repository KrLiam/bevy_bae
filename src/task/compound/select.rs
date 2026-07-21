//! Contains the [`Select`] [`CompoundTask`]

use std::ops::DerefMut;

use crate::{
    plan::PlanStep, prelude::*, task::{compound::{Decompose, DecomposeId, DecomposeInput, DecomposeResult, TaskTuple}, observer::ObserverOperatorSystems},
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
    In(input): In<DecomposeInput>,
    world: &mut World,
    mut d: Local<Decompose>,
    mut tasks_buffer: Local<Vec<TaskTuple>>,
) -> DecomposeResult {
    let d = d.deref_mut();

    let Ok(tasks) = d.q_task_lists.get(world, input.compound_task) else {
        return DecomposeResult::Failure;
    };
    d.get_tasks(world, tasks, &mut tasks_buffer);

    let backup_ctx = input.ctx_mut().clone();

    'task: for (
        i,
        (task_entity, has_enter, has_exit, has_operator, has_observer, compound_task),
    ) in tasks_buffer.drain(..).enumerate()
    {
        let (mtr, previous_mtr) = {
            let ctx = input.ctx_mut();
            let mtr = ctx.plan.mtr.clone().with(i as u16);
            let previous_mtr = ctx.previous_mtr.clone();
            (mtr, previous_mtr)
        };
        
        if mtr > previous_mtr {
            return DecomposeResult::Rejection;
        }
        
        if let Some(valid) = d.validate_conditions(world, task_entity, input.ctx_mut())
            && !valid
        {
            continue 'task;
        }

        if has_observer {
            let systems = world.resource::<ObserverOperatorSystems>();
            input.ctx_mut().plan.steps.push(PlanStep::RunSystem { entity: task_entity, system: Some(systems.enter), instant: true });
        }

        if has_enter {
            input.ctx_mut().plan.steps.push(PlanStep::RunEnterOperator {
                entity: task_entity,
                push_stack: has_exit
            });
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
                Ok(DecomposeResult::Failure) | Err(_) => {
                    input.ctx_mut().set_from(&backup_ctx);
                    continue;
                }
            }
        }

        if input.ctx_mut().plan.is_empty() {
            return DecomposeResult::Failure;
        }

        if has_observer {
            let systems = world.resource::<ObserverOperatorSystems>();
            input.ctx_mut().plan.steps.push(PlanStep::RunSystem { entity: task_entity, system: Some(systems.exit), instant: true });
        }
        
        d.apply_effects(world, task_entity, input.ctx_mut());
        
        if has_exit {
            input.ctx_mut().plan.steps.push(PlanStep::RunExitOperator(task_entity));
        }
        
        // only use the first match
        input.ctx_mut().plan.mtr.push(i as u16);
        return DecomposeResult::Success;
    }
    DecomposeResult::Failure
}
