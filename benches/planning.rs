//!


use std::time::Duration;

use bevy::{prelude::*, time::TimeUpdateStrategy};
use bevy_bae::{plan::update::update_plan_inner, prelude::*};
use bevy_ecs::system::SystemId;

fn plan() -> impl Bundle {
    (
        Plan::new(),
        Select,
        tasks! [
            (
                Name::new("a"),
                Sequence,
                tasks! [
                    op("a 1"),
                    op("a 2"),
                    op("a 3"),
                    op("a 4"),
                    op("a 5"),
                    op("a 6"),
                    op("a 7"),
                    op("a 8"),
                    op("a 9"),
                    (
                        op("a 10"),
                        conditions![Condition::eq("can_attack", true)]
                    ),
                ],
            ),
            (
                Name::new("b"),
                conditions![],
                Sequence,
                tasks! [
                    op("b 1"),
                    (
                        Select,
                        tasks! [
                            op_cond("b 2 1", cond("action", 1.0)),
                            op_cond("b 2 2", cond("action", 2.0)),
                            op_cond("b 2 3", cond("action", 3.0)),
                            op_cond("b 2 4", cond("action", 4.0)),
                            op("b 2 5"),
                        ],
                    ),
                    op("b 3"),
                ],
            )
        ],
    )
}

fn plan_empty() -> impl Bundle {
    (
        Plan::new(),
        Operator::noop()
    )
}

fn init_world<F, B>(w: &mut World, entities: u32, plan_fn: F)
where F: Fn() -> B, B: Bundle {

    let mut cmds = w.commands();
    for i in 0..entities {
        cmds.spawn((
            Name::new(format!("planner {i}")),
            plan_fn(),
        ));
    }

    w.flush();
}

type UpdateSystemId = SystemId<In<UpdatePlan>, Result<(), BevyError>>;

fn update(w: &mut World, system_id: UpdateSystemId) {
    let mut plans = w.query::<(Entity, &Plan)>();

    let entities = plans.iter(&w)
        .map(|(e, _)| e)
        .collect::<Vec<_>>();

    for &entity in &entities {
        let _ = w.run_system_with(system_id, UpdatePlan::new(entity));
    }
}

fn operator_system(_: impl Into<String>, duration: i32) -> impl FnMut(In<OperatorInput>, Local<i32>) -> OperatorStatus {
    move |
        _: In<OperatorInput>,
        mut counter: Local<i32>,
    | {
        *counter = *counter +1;
        if *counter <= duration - 1 {
            OperatorStatus::Ongoing
        }
        else {
            OperatorStatus::Success
        }
    }
}

fn op(name: &str) -> impl Bundle {
    let name = name.to_string();
    (
        Name::new(name.clone()),
        Operator::new(operator_system(name, 1)),
    )
}

fn op_cond(name: &str, cond: impl Bundle) -> impl Bundle {
    let name = name.to_string();
    (
        Name::new(name.clone()),
        Operator::new(operator_system(name, 1)),
        cond,
    )
}

fn cond(name: &str, val: impl Into<Value>) -> impl Bundle {
    conditions![Condition::eq(name, val)]
}

fn create_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        BaePlugin::default(),
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(
        Time::<Fixed>::default().timestep(),
    ));
    app.finish();
    app.update();
    app
}


use criterion::{criterion_group, criterion_main, Criterion};

const UPDATE_CALLS: usize = 1;
const ENTITIES: u32 = 1000;

fn criterion_benchmark(c: &mut Criterion) {
    // Read UPDATE_CALLS from environment or use default constant
    let update_calls: usize = std::env::var("UPDATE_CALLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(UPDATE_CALLS);
    let entities: u32 = std::env::var("ENTITIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(ENTITIES);

    let mut app_plan_empty = create_app();
    init_world(app_plan_empty.world_mut(), entities, plan_empty);
    let system_id = app_plan_empty.world_mut().register_system(update_plan_inner);

    c.bench_function("planning_empty", |b| {
        b.iter(|| {
            for _ in 0..update_calls {
                update(app_plan_empty.world_mut(), system_id);
            }
        })
    });

    let mut app_plan = create_app();
    init_world(app_plan.world_mut(), entities, plan);
    let system_id = app_plan.world_mut().register_system(update_plan_inner);

    c.bench_function("planning", |b| {
        b.iter(|| {
            for _ in 0..update_calls {
                update(app_plan.world_mut(), system_id);
            }
        })
    });
}

fn configure_criterion() -> Criterion {
    // Increase target time from 5.0s to 8.0s
    Criterion::default()
        .warm_up_time(Duration::from_secs(5))
        .measurement_time(Duration::from_secs(25))
        .sample_size(300)
}

criterion_group!(
    name = benches;
    config = configure_criterion();
    targets = criterion_benchmark
);
criterion_main!(benches);
