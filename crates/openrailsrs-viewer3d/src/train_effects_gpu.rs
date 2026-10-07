//! Hanabi executes plume motion on the GPU. The simulation remains the source
//! of emission, vehicle velocity and time; rain and snow share its simulation clock.
use bevy::prelude::*;
use bevy_hanabi::{
    Attribute, BoxedModifier, EffectAsset, EffectMaterial, EffectProperties, EffectSimulation,
    EffectSimulationTime, EffectSpawner, EffectSystems, ExprError, ExprWriter, HanabiPlugin,
    ImageSampleMapping, Modifier, ModifierContext, Module, MotionIntegration, OrientModifier,
    ParticleEffect, ParticleTextureModifier, RenderContext, RenderModifier, ScalarType,
    SetAttributeModifier, ShaderWriter, SimulationCondition, SimulationSpace, SpawnerSettings,
};
use std::collections::HashMap;

use crate::{live::LiveDrive, weather_execution::WeatherExecution};

pub(super) const MAX_GPU_EMITTERS: usize = 32;

#[derive(Resource, Default)]
pub(super) struct TrainParticleCapabilities {
    pub supported: bool,
    pub hardware: bool,
}

pub(super) struct TrainParticlesPlugin;
impl Plugin for TrainParticlesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TrainParticleCapabilities>();
        if app.get_sub_app(bevy::render::RenderApp).is_some() {
            app.add_plugins(HanabiPlugin);
            // Hanabi's First system normally advances from wall/virtual time.
            // Keep that clock paused and feed the actual railway clock instead.
            app.world_mut()
                .resource_mut::<Time<EffectSimulation>>()
                .pause();
            app.add_systems(PostUpdate, sync_clock.before(EffectSystems::TickSpawners));
        }
    }

    fn finish(&self, app: &mut App) {
        let Some(render) = app.get_sub_app(bevy::render::RenderApp) else {
            return;
        };
        let Some(device) = render
            .world()
            .get_resource::<bevy::render::renderer::RenderDevice>()
        else {
            return;
        };
        let limits = device.limits();
        let supported = limits.max_bind_groups >= 4
            && limits.max_storage_buffers_per_shader_stage >= 8
            && limits.max_compute_workgroups_per_dimension > 0;
        let hardware = render
            .world()
            .get_resource::<bevy::render::renderer::RenderAdapterInfo>()
            .is_some_and(|info| {
                matches!(
                    format!("{:?}", info.device_type).as_str(),
                    "DiscreteGpu" | "IntegratedGpu"
                )
            });
        app.insert_resource(TrainParticleCapabilities {
            supported,
            hardware,
        });
    }
}

fn sync_clock(
    live: Option<Res<LiveDrive>>,
    state: Res<State<crate::ViewerAppState>>,
    mut time: ResMut<Time<EffectSimulation>>,
    mut previous: Local<Option<f64>>,
) {
    let clock = live.as_ref().map(|l| l.session.time_s());
    let active =
        *state.get() == crate::ViewerAppState::Playing && live.as_ref().is_some_and(|l| !l.paused);
    let dt = simulation_delta(*previous, clock, active);
    *previous = clock;
    time.advance_by(std::time::Duration::from_secs_f64(dt));
}

fn simulation_delta(previous: Option<f64>, clock: Option<f64>, active: bool) -> f64 {
    match (previous, clock) {
        (Some(previous), Some(clock)) if active => (clock - previous).max(0.0),
        _ => 0.0,
    }
}

/// All emitters share one finite budget. Too many emitters or unsupported
/// compute use the existing merged CPU mesh, including on software adapters.
pub(super) fn resolve(
    requested: WeatherExecution,
    capabilities: Option<&TrainParticleCapabilities>,
    pressure: bool,
    emitter_count: usize,
) -> WeatherExecution {
    let gpu = capabilities.is_some_and(|c| c.supported && c.hardware)
        && (1..=MAX_GPU_EMITTERS).contains(&emitter_count);
    requested.resolved(gpu, pressure)
}

pub(super) fn budgets(mode: WeatherExecution, level: usize, total: usize) -> (usize, usize) {
    let total = total / [1, 2, 4][level.min(2)];
    match mode {
        WeatherExecution::Gpu | WeatherExecution::Auto => (total, 0),
        WeatherExecution::Hybrid => (total * 3 / 4, total / 4),
        WeatherExecution::Cpu => (0, total),
    }
}

#[derive(Component)]
pub(super) struct GpuExhaust;

pub(super) struct GpuEmitter {
    pub entity: Entity,
    pub capacity: usize,
}

pub(super) struct GpuAssets {
    pub texture: Handle<Image>,
    effects: HashMap<(bool, usize), Handle<EffectAsset>>,
}
impl GpuAssets {
    pub fn new(texture: Handle<Image>) -> Self {
        Self {
            texture,
            effects: HashMap::new(),
        }
    }
    pub fn handle(
        &mut self,
        assets: &mut Assets<EffectAsset>,
        steam: bool,
        capacity: usize,
    ) -> Handle<EffectAsset> {
        self.effects
            .entry((steam, capacity))
            .or_insert_with(|| assets.add(plume_asset(steam, capacity as u32)))
            .clone()
    }
}

pub(super) fn spawn(
    commands: &mut Commands,
    handle: Handle<EffectAsset>,
    texture: Handle<Image>,
    capacity: usize,
) -> GpuEmitter {
    let settings = SpawnerSettings::rate(0.0.into()).with_starts_active(false);
    let entity = commands
        .spawn((
            GpuExhaust,
            ParticleEffect::new(handle),
            EffectMaterial {
                images: vec![texture],
            },
            properties(Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, 0.1, 0.5, 0.7, 1.0),
            EffectSpawner::new(&settings),
            // This root is never attached to a carriage. Floating-origin shifts
            // translate it along with the scenery; old particles do not follow the
            // train, and spawn positions are converted into its anchored frame.
            Transform::IDENTITY,
            bevy::camera::visibility::NoFrustumCulling,
            Name::new("native train plume · Hanabi"),
        ))
        .id();
    GpuEmitter { entity, capacity }
}

pub(super) fn properties(
    position: Vec3,
    velocity: Vec3,
    wind: Vec3,
    radius: f32,
    shade: f32,
    opacity: f32,
    light: f32,
) -> EffectProperties {
    EffectProperties::default().with_properties([
        ("spawn_position".into(), position.into()),
        ("spawn_velocity".into(), velocity.into()),
        ("wind".into(), wind.into()),
        ("radius".into(), radius.into()),
        ("shade".into(), shade.into()),
        ("opacity".into(), opacity.into()),
        ("light".into(), light.into()),
    ])
}

/// Closed-form drag identical to the CPU path, with automatic Euler motion
/// disabled. Properties are per emitter; position/velocity/size/color live in
/// GPU storage. No per-particle readback or vertex upload is needed.
fn plume_asset(steam: bool, capacity: u32) -> EffectAsset {
    let writer = ExprWriter::new();
    let position = writer.add_property("spawn_position", Vec3::ZERO.into());
    let velocity = writer.add_property("spawn_velocity", Vec3::ZERO.into());
    let wind = writer.add_property("wind", Vec3::ZERO.into());
    let radius = writer.add_property("radius", 0.1_f32.into());
    let shade = writer.add_property("shade", 0.5_f32.into());
    let opacity = writer.add_property("opacity", 0.7_f32.into());
    let light = writer.add_property("light", 1.0_f32.into());
    writer.add_property("anchor", Vec3::ZERO.into());
    writer.add_property("fog", Vec4::ZERO.into());
    writer.add_property("ground_y", (-10000.0_f32).into());
    for i in 0..8 {
        writer.add_property(format!("solid_min_{i}"), Vec3::splat(10000.0).into());
        writer.add_property(format!("solid_max_{i}"), Vec3::splat(-10000.0).into());
    }
    let drag = if steam { 1.7 } else { 1.1 };
    // Hanabi initializes before updating in the same GPU frame. Starting age
    // at -delta and limiting the first step to zero keeps newborns at the
    // current native outlet, matching the CPU path even at accelerated time.
    let dt = writer
        .delta_time()
        .min(writer.attr(Attribute::AGE).max(writer.lit(0.0)));
    let equilibrium = writer.prop(wind) + writer.lit(Vec3::Y * (0.35 / drag));
    let relative = writer.attr(Attribute::VELOCITY) - equilibrium.clone();
    let decay = (writer.lit(-drag) * dt.clone()).exp();
    let turbulence = |age: bevy_hanabi::WriterExpr| {
        let phase = writer.attr(Attribute::F32_3);
        writer.lit(Vec3::X * 0.18) * (age.clone() * writer.lit(2.7) + phase.clone()).sin()
            + writer.lit(Vec3::Z * 0.16) * (age * writer.lit(1.9) + phase * writer.lit(1.3)).cos()
    };
    let age_now = writer.attr(Attribute::AGE).max(writer.lit(0.0));
    let turbulent_step = turbulence(age_now.clone()) - turbulence(age_now - dt.clone());
    let update_position = (writer.attr(Attribute::POSITION)
        + equilibrium.clone() * dt
        + relative.clone() * ((writer.lit(1.0) - decay.clone()) / writer.lit(drag))
        + turbulent_step)
        .expr();
    let update_velocity = (equilibrium + relative * decay).expr();
    let age = writer.attr(Attribute::AGE);
    let size = (writer.attr(Attribute::F32_0)
        * writer.lit(2.0)
        * (writer.lit(1.0) + age.clone() * writer.lit(4.0)))
    .expr();
    let fade = writer.lit(1.0) - age / writer.lit(super::train_effects::PUFF_LIFETIME_S);
    let rgb = writer.lit(Vec4::new(1.0, 1.0, 1.0, 0.0));
    let alpha = writer.lit(Vec4::new(0.0, 0.0, 0.0, 1.0));
    let color = (rgb.clone() * writer.attr(Attribute::F32_1) * writer.prop(light)
        + alpha.clone() * writer.attr(Attribute::F32_2) * fade.clone() * fade)
        .expr();
    let init_color =
        (rgb * writer.prop(shade) * writer.prop(light) + alpha * writer.prop(opacity)).expr();
    let initializers = [
        (
            Attribute::F32_3,
            (writer.rand(ScalarType::Float) * writer.lit(std::f32::consts::TAU)).expr(),
        ),
        (Attribute::POSITION, writer.prop(position).expr()),
        (Attribute::VELOCITY, writer.prop(velocity).expr()),
        (
            Attribute::AGE,
            (writer.delta_time() * writer.lit(-1.0)).expr(),
        ),
        (
            Attribute::LIFETIME,
            writer.lit(super::train_effects::PUFF_LIFETIME_S).expr(),
        ),
        (
            Attribute::F32_0,
            (writer.prop(radius)
                * (writer.lit(0.8) + writer.attr(Attribute::F32_3).sin().abs() * writer.lit(0.4)))
            .expr(),
        ),
        (Attribute::F32_1, writer.prop(shade).expr()),
        (Attribute::F32_2, writer.prop(opacity).expr()),
        (
            Attribute::SIZE,
            (writer.attr(Attribute::F32_0) * writer.lit(2.0)).expr(),
        ),
        (Attribute::HDR_COLOR, init_color),
    ];
    let previous_position = writer.attr(Attribute::POSITION).expr();
    let texture_slot = writer.lit(0_u32).expr();
    let mut module = writer.finish();
    module.add_texture_slot("plume");
    let mut effect = EffectAsset::new(capacity, SpawnerSettings::rate(0.0.into()), module)
        .with_name(if steam {
            "native steam"
        } else {
            "native diesel smoke"
        })
        .with_simulation_space(SimulationSpace::Local)
        .with_simulation_condition(SimulationCondition::Always)
        .with_motion_integration(MotionIntegration::None);
    for (attribute, value) in initializers {
        effect = effect.init(SetAttributeModifier::new(attribute, value));
    }
    effect
        .init(PlumeContact { initialize: true })
        .update(SetAttributeModifier::new(
            Attribute::F32X3_0,
            previous_position,
        ))
        .update(SetAttributeModifier::new(
            Attribute::POSITION,
            update_position,
        ))
        .update(SetAttributeModifier::new(
            Attribute::VELOCITY,
            update_velocity,
        ))
        .update(SetAttributeModifier::new(Attribute::SIZE, size))
        .update(SetAttributeModifier::new(Attribute::HDR_COLOR, color))
        .update(PlumeContact { initialize: false })
        .render(OrientModifier::default())
        .render(PlumeAtmosphere)
        .render(ParticleTextureModifier {
            texture_slot,
            sample_mapping: ImageSampleMapping::Modulate,
        })
}

/// Swept tests cover thin roofs even when simulation time is accelerated.
/// Bounds are conservative opaque parts; foliage and glass are excluded.
#[derive(Clone, Debug, Reflect)]
struct PlumeContact {
    initialize: bool,
}
impl Modifier for PlumeContact {
    fn context(&self) -> ModifierContext {
        if self.initialize {
            ModifierContext::Init
        } else {
            ModifierContext::Update
        }
    }
    fn attributes(&self) -> &[Attribute] {
        &[
            Attribute::POSITION,
            Attribute::F32X3_0,
            Attribute::U32_0,
            Attribute::AGE,
            Attribute::LIFETIME,
        ]
    }
    fn boxed_clone(&self) -> BoxedModifier {
        Box::new(self.clone())
    }
    fn apply(&self, _: &mut Module, context: &mut ShaderWriter) -> Result<(), ExprError> {
        if self.initialize {
            context
                .main_code
                .push_str("particle.u32_0 = properties_array_index;\n");
            return Ok(());
        }
        context.extra_code.push_str(
            r#"
fn plume_hits(a: vec3<f32>, b: vec3<f32>, lo: vec3<f32>, hi: vec3<f32>) -> bool {
    if any(lo > hi) { return false; }
    let delta = b - a;
    var enter = 0.0; var leave = 1.0;
    for (var i = 0u; i < 3u; i++) {
        if abs(delta[i]) < 0.000001 {
            if a[i] < lo[i] || a[i] > hi[i] { return false; }
        } else {
            let x = (lo[i] - a[i]) / delta[i]; let y = (hi[i] - a[i]) / delta[i];
            enter = max(enter, min(x,y)); leave = min(leave, max(x,y));
            if enter > leave { return false; }
        }
    }
    return true;
}
"#,
        );
        context.main_code.push_str("let pp = properties[properties_array_index];\nvar plume_blocked = particle.position.y < pp.ground_y;\n");
        for i in 0..8 {
            context.main_code.push_str(&format!("plume_blocked = plume_blocked || plume_hits(particle.f32x3_0, particle.position, pp.solid_min_{i}, pp.solid_max_{i});\n"));
        }
        context.main_code.push_str("if sim_params.delta_time > 0.0 && particle.age > 0.18 && plume_blocked { particle.age = particle.lifetime + 1.0; is_alive = false; }\n");
        Ok(())
    }
}

#[derive(Clone, Debug, Reflect)]
struct PlumeAtmosphere;
impl Modifier for PlumeAtmosphere {
    fn context(&self) -> ModifierContext {
        ModifierContext::Render
    }
    fn attributes(&self) -> &[Attribute] {
        &[Attribute::POSITION, Attribute::U32_0, Attribute::F32_3]
    }
    fn boxed_clone(&self) -> BoxedModifier {
        Box::new(self.clone())
    }
    fn as_render(&self) -> Option<&dyn RenderModifier> {
        Some(self)
    }
    fn as_render_mut(&mut self) -> Option<&mut dyn RenderModifier> {
        Some(self)
    }
    fn into_boxed_render(self: Box<Self>) -> Option<Box<dyn RenderModifier>> {
        Some(self)
    }
    fn apply(&self, _: &mut Module, _: &mut ShaderWriter) -> Result<(), ExprError> {
        Ok(())
    }
}
impl RenderModifier for PlumeAtmosphere {
    fn boxed_render_clone(&self) -> Box<dyn RenderModifier> {
        Box::new(self.clone())
    }
    fn as_modifier(&self) -> &dyn Modifier {
        self
    }
    fn apply_render(&self, _: &mut Module, context: &mut RenderContext) -> Result<(), ExprError> {
        context.vertex_code.push_str("let plume_angle = particle.f32_3;\nlet plume_right = axis_x;\naxis_x = plume_right * cos(plume_angle) + axis_y * sin(plume_angle);\naxis_y = -plume_right * sin(plume_angle) + axis_y * cos(plume_angle);\n");
        context.vertex_code.push_str("let pp = properties[particle.u32_0];\nlet plume_world = particle.position + pp.anchor;\nlet air_transmission = exp(-pp.fog.w * distance(plume_world, view.world_from_view[3].xyz));\ncolor = vec4(mix(pp.fog.xyz, color.rgb, air_transmission), color.a);\n");
        Ok(())
    }
}

pub(super) fn environment(
    properties: &mut EffectProperties,
    anchor: Vec3,
    ground_y: f32,
    fog: Vec4,
    solids: &[crate::effect_obstacles::Solid],
) {
    properties.set("anchor", anchor.into());
    properties.set("ground_y", (ground_y - anchor.y).into());
    properties.set("fog", fog.into());
    for i in 0..8 {
        let (min, max) = solids
            .get(i)
            .map_or((Vec3::splat(10000.0), Vec3::splat(-10000.0)), |s| {
                (s.min - anchor, s.max - anchor)
            });
        properties.set(&format!("solid_min_{i}"), min.into());
        properties.set(&format!("solid_max_{i}"), max.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_uses_simulation_steps_and_freezes_pause_menu_and_restart() {
        assert_eq!(simulation_delta(Some(10.0), Some(10.4), true), 10.4 - 10.0);
        assert_eq!(simulation_delta(Some(10.0), Some(12.0), false), 0.0);
        assert_eq!(simulation_delta(Some(10.0), Some(0.0), true), 0.0);
        assert_eq!(simulation_delta(None, Some(12.0), true), 0.0);
        assert_eq!(simulation_delta(Some(12.0), None, true), 0.0);
    }
    #[test]
    fn hanabi_clock_resource_follows_live_session_including_pause_and_rewind() {
        use bevy::ecs::system::RunSystemOnce;
        let mut app = App::new();
        let live =
            LiveDrive::from_scenario_path(&crate::test_harness::smoke_scenario_path()).unwrap();
        app.insert_resource(live)
            .insert_resource(State::new(crate::ViewerAppState::Playing))
            .init_resource::<Time<EffectSimulation>>()
            .add_systems(PostUpdate, sync_clock);
        app.update();
        let clock = app.world().resource::<LiveDrive>().session.time_s();
        app.world_mut()
            .resource_mut::<LiveDrive>()
            .session
            .step_realtime(0.4, |_| {});
        let expected = app.world().resource::<LiveDrive>().session.time_s() - clock;
        app.update();
        assert!(
            (app.world()
                .resource::<Time<EffectSimulation>>()
                .delta_secs_f64()
                - expected)
                .abs()
                < 1e-9
        );
        app.world_mut().resource_mut::<LiveDrive>().paused = true;
        app.update();
        assert_eq!(
            app.world()
                .resource::<Time<EffectSimulation>>()
                .delta_secs_f64(),
            0.0
        );
        app.world_mut().resource_mut::<LiveDrive>().reset().unwrap();
        app.update();
        assert_eq!(
            app.world()
                .resource::<Time<EffectSimulation>>()
                .delta_secs_f64(),
            0.0
        );
        // RunSystemOnce has a fresh Local; entering a new game starts with zero.
        app.world_mut().run_system_once(sync_clock).unwrap();
        assert_eq!(
            app.world()
                .resource::<Time<EffectSimulation>>()
                .delta_secs_f64(),
            0.0
        );
    }
    #[test]
    fn policy_has_compute_fallback_and_one_combined_budget() {
        let gpu = TrainParticleCapabilities {
            supported: true,
            hardware: true,
        };
        let software = TrainParticleCapabilities {
            supported: true,
            hardware: false,
        };
        let unsupported = TrainParticleCapabilities {
            supported: false,
            hardware: true,
        };
        for requested in [
            WeatherExecution::Auto,
            WeatherExecution::Gpu,
            WeatherExecution::Cpu,
            WeatherExecution::Hybrid,
        ] {
            assert_eq!(resolve(requested, None, false, 2), WeatherExecution::Cpu);
            assert_eq!(
                resolve(requested, Some(&software), false, 2),
                WeatherExecution::Cpu
            );
            assert_eq!(
                resolve(requested, Some(&unsupported), false, 2),
                WeatherExecution::Cpu
            );
            assert_eq!(
                resolve(requested, Some(&gpu), false, 33),
                WeatherExecution::Cpu
            );
        }
        assert_eq!(
            resolve(WeatherExecution::Auto, Some(&gpu), false, 2),
            WeatherExecution::Gpu
        );
        assert_eq!(
            resolve(WeatherExecution::Auto, Some(&gpu), true, 2),
            WeatherExecution::Hybrid
        );
        for level in 0..3 {
            for mode in [
                WeatherExecution::Gpu,
                WeatherExecution::Cpu,
                WeatherExecution::Hybrid,
            ] {
                let (gpu, cpu) = budgets(mode, level, 512);
                assert_eq!(gpu + cpu, 512 / (1 << level));
            }
        }
    }
    #[test]
    fn both_native_plumes_generate_valid_hanabi_shader_sources() {
        for steam in [false, true] {
            let asset = plume_asset(steam, 64);
            assert_eq!(asset.capacity(), 64);
            assert_eq!(asset.motion_integration, MotionIntegration::None);
            assert_eq!(asset.simulation_space, SimulationSpace::Local);
            bevy_hanabi::EffectShaderSources::generate(&asset, None, 0).unwrap();
        }
    }
}
