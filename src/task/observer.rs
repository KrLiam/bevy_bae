//! Contains [`ObserverOperator`] for implementing operators
//! that listen for events.

use std::sync::Arc;

use bevy_ecs::{component::Component, entity::Entity, event::Event, lifecycle::HookContext, observer::{Observer, On}, query::QueryState, resource::Resource, system::{Commands, In, IntoSystem, Local}, world::{DeferredWorld, World}};
use smallvec::{SmallVec, smallvec};
use tracing::debug;

use crate::{plan::Plan, task::{OperatorId, OperatorInput, OperatorStatus}};

/// Stores the observers registered by an [`ObserverOperator`].
#[derive(Debug)]
pub struct ObserverGroup {
    /// The entity containing [`ObserverOperator`].
    pub operator: Entity,
    /// The observer entities.
    pub observers: SmallVec<[Entity; 4]>,
    /// Whether any of the observers returned [`OperatorStatus::Success`].
    pub completed: bool
}

/// The state of a [`Plan`] for handling observers.
#[derive(Debug, Component, Default)]
pub struct PlanObservers {
    /// The state of all [`ObserverOperator`]s currently in execution.
    pub stack: Vec<ObserverGroup>
}


type SpawnFn = Arc<dyn Fn(OperatorInput, &mut World) + Send + Sync>;


/// An operator that registers an observer that listens
/// for events. If the event is [`EntityEvent`], the observer
/// is triggered only if the event target is the entity with [`Plan`].
/// 
/// The observer is active only during the execution of the task. If
/// the main operator (for primitive tasks) or subtasks (for compound tasks)
/// finishes, execution hangs until the observer system have been triggered
/// returned [`OperatorStatus::Success`] at least once.
#[derive(Component)]
#[component(on_insert=Self::on_insert_hook)]
pub struct ObserverOperator {
    /// The function that registers the observer system.
    register: Option<Box<dyn FnOnce(&mut Commands) -> SpawnFn + Send + Sync>>,
    /// The function that spawn the observers.
    spawn: Option<SpawnFn>,
}

impl ObserverOperator {
    /// Returns a [`ObserverOperator`] that listens for `E`.
    pub fn new<S, E, M>(system: S) -> Self
    where
        S: IntoSystem<In<(E, OperatorInput)>, OperatorStatus, M>,
        S::System: Send + Sync + 'static,
        E: Send + Sync + 'static,
        E: Event + Clone,
    {
        let system = IntoSystem::into_system(system);
        Self {
            // the function that register the system, runs once when ObserverOperator is inserted in the plan.
            register: Some(Box::new(move |cmds: &mut Commands<'_, '_>| {
                // register the system
                let observer_id = cmds.register_system(system);
                debug!(?observer_id, "registered observer");

                // the spawn function. this is ran when the plan execution reaches this operator.
                // at this point, we know the executor entity.
                Arc::new(move |op_input, world| {
                    // spawn the observer entity attached to the executor entity.
                    let observer = world.spawn(Observer::new(move |event: On<E>, mut cmds: Commands| {
                        let input = (event.clone(), op_input);
                        debug!(executor=?op_input.entity, "operator observer called.");
                        // call an exclusive system to be able to run the observer system
                        // and read the returned OperatorStatus.
                        cmds.queue(move |w: &mut World| {
                            debug!(executor=?op_input.entity, "running observer operator.");
                            let result = w.run_system_with(observer_id, input);
                            // process result like `execute_plan`
                            match result {
                                Ok(OperatorStatus::Success) => {
                                    debug!(
                                        ?op_input.entity,
                                        "observer operator system returned success. marking as complete."
                                    );
                                    let mut entity_ref = w.entity_mut(op_input.entity);
                                    let Ok((
                                        mut obs,
                                        mut plan
                                    )) = entity_ref.get_components_mut::<(&mut PlanObservers, &mut Plan)>()
                                    else { return };

                                    plan.paused = false;

                                    let Some(g) = obs.stack.iter_mut()
                                        .find(|g| g.operator == op_input.operator)
                                    else { return };
                                    g.completed = true;

                                },
                                Ok(OperatorStatus::Ongoing) => {
                                    debug!(
                                        ?op_input.entity,
                                        "observer operator system is ongoing"
                                    );
                                    return
                                },
                                Ok(OperatorStatus::Failure) => {
                                    debug!(
                                        ?op_input.entity,
                                        "observer operator system failed, aborting plan"
                                    );
                                    let mut plan = w.get_mut::<Plan>(op_input.entity).unwrap();
                                    plan.clear();
                                }
                                Err(err) => {
                                    debug!(
                                        ?op_input.entity,
                                        ?err,
                                        "observer operator system failed, aborting plan"
                                    );
                                    let mut plan = w.get_mut::<Plan>(op_input.entity).unwrap();
                                    plan.clear();
                                },
                            }
                        });
                    }).with_entity(op_input.entity)).id();

                    let mut entity_ref = world.entity_mut(op_input.entity);
                    let mut plan_observers = entity_ref
                        .entry::<PlanObservers>()
                        .or_insert(Default::default());
                    let mut plan_observers = plan_observers.get_mut();
                    plan_observers.stack.push(ObserverGroup {
                        operator: op_input.operator,
                        observers: smallvec! [ observer ],
                        completed: false,
                    });
                    debug!(?observer, executor=?op_input.entity, "spawned operator observer.\n{plan_observers:?}");
                })
            })),
            spawn: None,
        }
    }

    fn on_insert_hook(mut world: DeferredWorld, context: HookContext) {
        let Some(register_system) = world
            .get_mut::<Self>(context.entity)
            .and_then(|mut op| op.register.take())
        else {
            return;
        };
        let spawn_fn = register_system(&mut world.commands());
        world.get_mut::<Self>(context.entity).unwrap().spawn = Some(spawn_fn);
    }

    pub(crate) fn on_enter(
        input: In<OperatorInput>,
        world: &mut World,
        mut operators: Local<QueryState<&ObserverOperator>>,
        // mut plan_observers: Local<QueryState<&mut PlanObservers>>,
    ) -> OperatorStatus {
        let Ok(obs) = operators.get(world, input.operator)
        else { return OperatorStatus::Success };
        let Some(spawn) = obs.spawn.clone()
        else { return OperatorStatus::Success };

        debug!(executor=?input.entity, operator=?input.operator, "Entering observer operator");

        // FnMut(&mut World, Entity)
        // let mut f = |world: &mut World, observer: Entity| {
        //     let Ok(mut c) = plan_observers.get_mut(world, input.entity) else { return };
        //     c.stack.push(ObserverGroup {
        //         operator: input.operator,
        //         observers: smallvec! [ observer ],
        //         completed: false,
        //     });
        // };
        spawn(*input, world);
        world.flush();

        OperatorStatus::Success
    }

    pub(crate) fn on_exit(
        input: In<OperatorInput>,
        world: &mut World,
        mut observers: Local<QueryState<(&mut PlanObservers, &mut Plan)>>,
    ) -> OperatorStatus {
        let Ok((mut plan_observers, mut plan)) = observers.get_mut(world, input.entity)
        else { return OperatorStatus::Success };

        let Some(i) = plan_observers.stack.iter()
            .position(|g| g.operator == input.operator)
        else { return OperatorStatus::Success };

        let group = &plan_observers.stack[i];
        if !group.completed {
            debug!(
                executor=?input.entity,
                operator=?input.operator,
                "Observer operator not complete yet. Pausing plan",
            );
            plan.paused = true;
            return OperatorStatus::Ongoing
        }

        let group = plan_observers.stack.remove(i);

        debug!(
            executor=?input.entity,
            operator=?input.operator,
            "Exiting observer operator, despawning {} observers.",
            group.observers.len()
        );

        for entity in group.observers {
            world.entity_mut(entity).despawn();
        }

        OperatorStatus::Success
    }
}

#[derive(Resource)]
pub(crate) struct ObserverOperatorSystems {
    pub(crate) enter: OperatorId,
    pub(crate) exit: OperatorId,
}

pub(crate) fn register_observer_operator_systems(world: &mut World) {
    let enter = world.register_system(ObserverOperator::on_enter);
    let exit = world.register_system(ObserverOperator::on_exit);
    println!("Registered observer operator systems.");
    world.insert_resource(ObserverOperatorSystems { enter, exit });
}
