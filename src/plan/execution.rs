use crate::{plan::{CheckStep, PlanReactivity, PlanScope, PlanStep}, prelude::*, task::scope::{EnterOperator, ExitOperator}};


pub(crate) fn check_plan_on_prop_change(
    entities: Query<(Entity, &Plan,&Props, &PlanReactivity), Changed<Props>>,
    mut cmds: Commands,
) {
    for (entity, plan, props, react) in entities {
        // if plan is empty, 'update_empty_plans' will take care of it
        if plan.is_empty() {
            continue
        }

        for step in &react.check_steps {
            match step {
                CheckStep::Condition { condition, expected } => {
                    let new_result = condition.is_fullfilled(props);
                    if new_result != *expected {
                        cmds.entity(entity).trigger(UpdatePlan::new);
                        debug!("Entity {} triggered replanning due to prop change: {:?}", entity, condition);
                        break
                    }
                },
                CheckStep::Effects { entity: _ } => {},
            }
        }
    }
}

pub(crate) fn update_empty_plans(
    mut plans: Query<(Entity, NameOrEntity, &Plan)>,
    mut commands: Commands,
) {
    for (entity, name, plan) in plans.iter_mut() {
        if plan.is_empty() {
            commands.entity(entity).trigger(UpdatePlan::new);
            debug!(entity=?name.entity, name=?name.name, "Plan is empty, triggering replan.");
        }
    }
}

pub(crate) fn execute_plan(
    world: &mut World,
    mut q_plans: Local<QueryState<(NameOrEntity, &mut Plan, &mut PlanScope)>>,
    mut q_conditions_list: Local<QueryState<&Conditions>>,
    mut q_conditions: Local<QueryState<(&Condition, NameOrEntity)>>,
    mut q_operators: Local<QueryState<(NameOrEntity, &Operator)>>,
    mut q_enter_operators: Local<QueryState<(NameOrEntity, &EnterOperator)>>,
    mut q_exit_operators: Local<QueryState<(NameOrEntity, &ExitOperator)>>,
    mut q_effects_list: Local<QueryState<&Effects>>,
    mut q_effects: Local<QueryState<(NameOrEntity, &Effect)>>,
    mut plans_buffer: Local<Vec<(Entity, Option<Name>, Option<OperatorStatus>)>>,
    mut effects_buffer: Local<Vec<(Entity, Option<Name>, Effect)>>,
) {
    plans_buffer.extend(
        q_plans.iter(world).map(|(name, plan, _)| {
            (name.entity, name.name.cloned(), plan.status)
        }),
    );
    for (plan_entity, plan_name, _) in plans_buffer.drain(..) {
        let mut ran_operator = false;

        'plan_loop: loop {
            let Some(step) = q_plans.get(world, plan_entity)
                .ok()
                .and_then(|(_, plan, _)| plan.current_step())
            else { break 'plan_loop };
    
            let mut status = OperatorStatus::Success;
            let mut skip_advance = false;
    
            match step {
                PlanStep::ValidateConditions(entity) => {
                    debug!(?plan_entity, ?plan_name, "checking conditions");
                    
                    let entity_ref = world.entity(plan_entity);
                    let props = entity_ref.props();
                    
                    let Ok(c) = q_conditions_list.get(world, entity) else { continue };
                    let conditions = q_conditions.iter_many(world, c);
    
                    for (condition, name) in conditions {
                        if condition.is_fullfilled(&props) {
                            debug!(
                                ?plan_entity,
                                ?plan_name,
                                condition_entity=?name.entity,
                                condition_name=?name,
                                "satisfied condition"
                            );
                        } else {
                            debug!(
                                ?plan_entity,
                                ?plan_name,
                                condition_entity=?name.entity,
                                condition_name=?name,
                                "encountered unsatisfied condition, aborting plan"
                            );
                            status = OperatorStatus::Failure;
                            break;
                        }
                    }
                },
                PlanStep::RunOperator(entity) => {
                    let input = OperatorInput {
                        entity: plan_entity,
                        operator: entity,
                    };
    
                    if let Ok((op_name, operator)) = q_operators.get(world, entity) {                
                        debug!(
                            ?plan_entity,
                            ?plan_name,
                            operator_entity=?op_name.entity,
                            operator_name=?op_name.name,
                            "running operator"
                        );
                        let r = world.run_system_with(operator.system_id(), input);
                        world.flush();
    
                        match r {
                            Ok(r) => {
                                status = r;
                            },
                            Err(err) => {
                                debug!(
                                    ?plan_entity,
                                    ?plan_name,
                                    ?err,
                                    "operator system failed, aborting plan"
                                );
                                status = OperatorStatus::Failure;
                            },
                        }
                    }
                    else {
                        debug!(
                            operator_entity=?entity,
                            "failed to find operator"
                        );
                        status = OperatorStatus::Failure;
                    }
                },
                PlanStep::RunEnterOperator { entity, push_stack } => {
                    if push_stack {
                        let Ok((_, _, mut scope)) = q_plans.get_mut(world, plan_entity)
                        else { panic!() };
                        scope.push(entity);
                    }

                    let input = OperatorInput {
                        entity: plan_entity,
                        operator: entity,
                    };
                    if let Ok((op_name, operator)) = q_enter_operators.get(world, entity)
                        && let Some(system_id) = operator.system_id()
                    {                
                        debug!(
                            ?plan_entity,
                            ?plan_name,
                            operator_entity=?op_name.entity,
                            operator_name=?op_name.name,
                            "running enter operator"
                        );
                        let _ = world.run_system_with(system_id, input);
                        world.flush();
                    }
                }
                PlanStep::RunExitOperator(entity) => {
                    let input = OperatorInput {
                        entity: plan_entity,
                        operator: entity,
                    };
                    if let Ok((op_name, operator)) = q_exit_operators.get(world, entity)
                        && let Some(system_id) = operator.system_id()
                    {                
                        debug!(
                            ?plan_entity,
                            ?plan_name,
                            operator_entity=?op_name.entity,
                            operator_name=?op_name.name,
                            "running enter operator"
                        );
                        let _ = world.run_system_with(system_id, input);
                        world.flush();
                    }

                    let Ok((_, _, mut scope)) = q_plans.get_mut(world, plan_entity)
                    else { panic!() };
                    scope.pop(entity);
                }
                PlanStep::ApplyEffects(entity) => {
                    let Ok(c) = q_effects_list.get(world, entity) else { continue };
                    effects_buffer.extend(
                        q_effects
                            .iter_many(world, c)
                            .map(|(name, effect)| (name.entity, name.name.cloned(), effect.clone())),
                    );
    
                    let mut entity_ref = world.entity_mut(plan_entity);
                    let mut props = entity_ref.props_mut();
                    
                    for (effect_entity, effect_name, effect) in effects_buffer.drain(..) {
                        if effect.plan_only {
                            debug!(
                                ?plan_entity,
                                ?plan_name,
                                ?effect_entity,
                                ?effect_name,
                                "skipping effect as it's plan_only"
                            );
                        } else {
                            debug!(
                                ?plan_entity,
                                ?plan_name,
                                ?effect_entity,
                                ?effect_name,
                                "applying effect"
                            );
                            effect.apply(&mut props);
                        }
                    }
                },
                PlanStep::Jump { index, .. } => {
                    debug!(
                        ?plan_entity,
                        ?plan_name,
                        "jumping to index {index}"
                    );

                    let Ok((_, mut plan, _)) = q_plans.get_mut(world, plan_entity)
                    else { panic!() };
                    plan.index = usize::min(index, plan.steps.len().saturating_sub(1));
                    skip_advance = true;
                }
            }
    
            match status {
                OperatorStatus::Success => {
                    let Ok((_, mut plan, _)) = q_plans.get_mut(world, plan_entity)
                    else { panic!() };
    
                    debug!(
                        ?plan_entity,
                        ?plan_name,
                        "step completed successfully, moving to next step"
                    );
                    if !skip_advance {
                        plan.advance();
                    }
                }
                OperatorStatus::Ongoing => {
                    debug!(?plan_entity, ?plan_name, "operator ongoing");
                }
                OperatorStatus::Failure => {
                    debug!(?plan_entity, ?plan_name, "step failed, aborting plan");
                }
            }
    
            let mut plan = q_plans.get_mut(world, plan_entity)
                .map(|(_, plan, _)| plan)
                .unwrap();
            plan.status = Some(status);
            debug!("plan status is {:?}", plan.status);
    
            let failed = status == OperatorStatus::Failure;
            let next_step = plan.current_step();
            
            if failed || next_step.is_none() {
                world.entity_mut(plan_entity).insert(Plan::default());
                debug!(?plan_entity, ?plan_name, "triggering replan");
                break 'plan_loop
            }
            
            // check whether the next step should execute now
            ran_operator = ran_operator || matches!(step, PlanStep::RunOperator(_));
            let can_run_next_step = matches!(next_step, Some(
                PlanStep::ApplyEffects(_) | PlanStep::RunExitOperator(_) | PlanStep::Jump { .. }
            ));
            let run_next_step = !ran_operator || can_run_next_step;
            if run_next_step {
                continue
            }

            break 'plan_loop
        }
    }
}

pub fn run_exit_operators_on_inserted_plan(
    event: On<Insert, Plan>,
    mut plans: Query<(&Plan, &mut PlanScope)>,
    exit_operators: Query<&ExitOperator>,
    mut cmds: Commands,
) {
    let Ok((_plan, mut scope)) = plans.get_mut(event.entity) else { return };

    while let Some(operator_entity) = scope.stack.pop() {
        let Ok(operator) = exit_operators.get(operator_entity) else { continue };
        let Some(system_id) = operator.system_id() else { continue };

        let input = OperatorInput {
            entity: event.entity,
            operator: operator_entity
        };
        cmds.run_system_with(system_id, input);
    }
}

pub fn run_exit_operators_on_removed_plan(
    event: On<Remove, Plan>,
    mut plans: Query<(&Plan, &mut PlanScope)>,
    exit_operators: Query<&ExitOperator>,
    mut cmds: Commands,
) {
    let Ok((_plan, mut scope)) = plans.get_mut(event.entity) else { return };

    while let Some(operator_entity) = scope.stack.pop() {
        let Ok(operator) = exit_operators.get(operator_entity) else { continue };
        let Some(system_id) = operator.system_id() else { continue };

        let input = OperatorInput {
            entity: event.entity,
            operator: operator_entity
        };
        cmds.run_system_with(system_id, input);
    }
}