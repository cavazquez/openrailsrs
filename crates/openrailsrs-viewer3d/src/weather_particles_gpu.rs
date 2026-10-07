//! Persistent Hanabi precipitation. A deterministic seed lives in GPU storage;
//! the railway clock, wrapping field and silhouettes match the CPU fallback.
use bevy::prelude::*;
use bevy_hanabi::{
    Attribute, BoxedModifier, EffectAsset, EffectMaterial, EffectProperties, EffectSpawner,
    ExprError, ExprWriter, Modifier, ModifierContext, Module, MotionIntegration, ParticleEffect,
    RenderContext, RenderModifier, ShaderWriter, SimulationCondition, SimulationSpace,
    SpawnerSettings,
};
use std::collections::HashMap;

use crate::weather_particles::{ParticleUniforms, WeatherMesh};

#[derive(Component)]
pub(super) struct HanabiWeather;

#[derive(Default)]
pub(super) struct GpuWeather {
    pub entity: Option<Entity>,
    pub configuration: Option<(bool, usize)>,
    pub seed_initializations: u64,
    seeded: bool,
    assets: HashMap<(bool, usize), Handle<EffectAsset>>,
}

impl GpuWeather {
    pub fn disable(&mut self, commands: &mut Commands) {
        if let Some(entity) = self.entity.take() {
            commands.entity(entity).despawn();
        }
        self.configuration = None;
        self.seeded = false;
    }

    pub fn configure(
        &mut self,
        commands: &mut Commands,
        assets: &mut Assets<EffectAsset>,
        shelter: Handle<Image>,
        snow: bool,
        count: usize,
    ) {
        let capacity = if count == 0 {
            0
        } else {
            count.next_power_of_two().clamp(128, 8192)
        };
        if self.configuration == Some((snow, capacity)) {
            return;
        }
        self.disable(commands);
        if count == 0 {
            return;
        }
        // Quantized quality budgets keep the cache bounded. Provider intensity
        // changes only the visible quota, never create a new effect asset.
        let handle = self
            .assets
            .entry((snow, capacity))
            .or_insert_with(|| assets.add(weather_asset(snow, capacity as u32)));
        let settings = SpawnerSettings::rate(0.0.into()).with_starts_active(false);
        self.entity = Some(
            commands
                .spawn((
                    HanabiWeather,
                    WeatherMesh,
                    ParticleEffect::new(handle.clone()),
                    EffectSpawner::new(&settings),
                    EffectMaterial {
                        images: vec![shelter],
                    },
                    properties(
                        &ParticleUniforms::default(),
                        0.0,
                        0.0,
                        Vec4::ZERO,
                        Vec4::ZERO,
                        Vec4::ZERO,
                        count,
                    ),
                    Transform::IDENTITY,
                    bevy::camera::visibility::NoFrustumCulling,
                    Name::new(if snow {
                        "snow · Hanabi"
                    } else {
                        "rain · Hanabi"
                    }),
                ))
                .id(),
        );
        self.configuration = Some((snow, capacity));
    }

    pub fn seed_once(&mut self, spawner: &mut EffectSpawner, ready: bool) {
        spawner.spawn_count = 0;
        if !self.seeded
            && ready
            && let Some((_, count)) = self.configuration
        {
            spawner.spawn_count = count.next_power_of_two().clamp(128, 8192) as u32;
            self.seeded = true;
            self.seed_initializations += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn properties(
    p: &ParticleUniforms,
    shelter_base: f32,
    light: f32,
    fog: Vec4,
    lamp_position: Vec4,
    lamp_direction: Vec4,
    visible_count: usize,
) -> EffectProperties {
    EffectProperties::default().with_properties([
        ("center".into(), p.center.into()),
        ("phase".into(), p.phase.into()),
        ("right".into(), p.right.into()),
        ("up".into(), p.up.into()),
        ("wind_time".into(), p.wind_time.into()),
        ("grid".into(), p.grid.into()),
        ("shelter_base".into(), shelter_base.into()),
        ("light".into(), light.into()),
        ("fog".into(), fog.into()),
        ("lamp_position".into(), lamp_position.into()),
        ("lamp_direction".into(), lamp_direction.into()),
        ("visible_count".into(), (visible_count as u32).into()),
    ])
}

const ATTRIBUTES: &[Attribute] = &[
    Attribute::POSITION,
    Attribute::F32X4_0,
    Attribute::F32X4_1,
    Attribute::U32_0,
    Attribute::U32_1,
];

#[derive(Clone, Debug, Reflect)]
struct WeatherMotion {
    init: bool,
    snow: bool,
}
impl Modifier for WeatherMotion {
    fn context(&self) -> ModifierContext {
        if self.init {
            ModifierContext::Init
        } else {
            ModifierContext::Update
        }
    }
    fn attributes(&self) -> &[Attribute] {
        ATTRIBUTES
    }
    fn boxed_clone(&self) -> BoxedModifier {
        Box::new(self.clone())
    }
    fn apply(&self, _: &mut Module, context: &mut ShaderWriter) -> Result<(), ExprError> {
        if self.init {
            context.extra_code.push_str(
                r#"
fn weather_hash(seed_value: u32, channel: u32) -> f32 {
    var x = seed_value * 0x9E3779B9u ^ channel * 0x85EBCA6Bu;
    x = x ^ (x >> 16u); x = x * 0x7FEB352Du; x = x ^ (x >> 16u);
    return f32(x) / 4294967295.0;
}
"#,
            );
            context.main_code.push_str(r#"
let weather_seed = particle_counter + 1u;
particle.f32x4_0 = vec4(weather_hash(weather_seed, 0u), weather_hash(weather_seed, 1u), weather_hash(weather_seed, 2u), weather_hash(weather_seed, 3u));
particle.u32_0 = properties_array_index;
particle.u32_1 = particle_counter;
"#);
        }
        context.extra_code.push_str("fn weather_wrap(v: f32, span: f32) -> f32 { return v - floor((v + span * 0.5) / span) * span; }\n");
        let snow = self.snow;
        context.main_code.push_str(&format!(r#"
let wp = properties[properties_array_index];
particle.f32x4_1 = vec4(wp.grid.xyz, wp.shelter_base);
let ws = particle.f32x4_0;
let wt = wp.wind_time.w;
let fall = {fall};
let flutter = {flutter};
let drift = select(wt, 1.0, wp.wind_time.y > 0.5);
let span = wp.center.w * 2.0;
let raw = vec3(ws.x * span + wp.wind_time.x * drift + flutter.x,
    ws.y * wp.phase.w - fall * wt,
    ws.z * span + wp.wind_time.z * drift + flutter.y) - wp.phase.xyz;
particle.position = wp.center.xyz + vec3(weather_wrap(raw.x, span), weather_wrap(raw.y, wp.phase.w), weather_wrap(raw.z, span));
"#,
            fall = if snow { "0.7 + ws.w * 1.2" } else { "20.0 + ws.w * 16.0" },
            flutter = if snow { "vec2(sin(wt * 0.73 + ws.x * 31.0), cos(wt * 0.51 + ws.z * 27.0)) * 0.65" } else { "vec2(0.0)" },
        ));
        Ok(())
    }
}

#[derive(Clone, Debug, Reflect)]
struct WeatherRender {
    snow: bool,
}
impl Modifier for WeatherRender {
    fn context(&self) -> ModifierContext {
        ModifierContext::Render
    }
    fn attributes(&self) -> &[Attribute] {
        ATTRIBUTES
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
impl RenderModifier for WeatherRender {
    fn boxed_render_clone(&self) -> Box<dyn RenderModifier> {
        Box::new(self.clone())
    }
    fn as_modifier(&self) -> &dyn Modifier {
        self
    }
    fn apply_render(&self, _: &mut Module, context: &mut RenderContext) -> Result<(), ExprError> {
        context.set_needs_uv();
        context.set_needs_particle_fragment();
        context
            .vertex_code
            .push_str("let wp = properties[particle.u32_0];\n");
        if self.snow {
            context.vertex_code.push_str(
                r#"
let angle = particle.f32x4_0.w * tau + wp.wind_time.w * (0.35 + particle.f32x4_0.x);
axis_x = wp.right.xyz * cos(angle) + wp.up.xyz * sin(angle);
axis_y = -wp.right.xyz * sin(angle) + wp.up.xyz * cos(angle);
size = vec3(vec2(0.036 + particle.f32x4_0.w * 0.080), 1.0);
"#,
            );
        } else {
            context.vertex_code.push_str("axis_x = wp.right.xyz; axis_y = vec3(0.0, 1.0, 0.0); size = vec3(0.025, 0.8 + particle.f32x4_0.w * 0.65, 1.0);\n");
        }
        context.vertex_code.push_str(r#"
let weather_world = particle.position;
let radial = 1.0 - smoothstep(wp.center.w * 0.72, wp.center.w, length(weather_world.xz - wp.center.xz));
let to_drop = weather_world - wp.lamp_position.xyz;
let distance2 = max(dot(to_drop, to_drop), 1.0);
let cone = smoothstep(wp.lamp_direction.w, min(0.9999, wp.lamp_direction.w + 0.035), dot(normalize(to_drop), wp.lamp_direction.xyz));
let beam = min(1.8, wp.lamp_position.w / distance2) * cone;
let air = exp(-wp.fog.w * distance(weather_world, view.world_from_view[3].xyz));
color = vec4(mix(wp.fog.rgb, vec3(0.86, 0.91, 0.96) * (wp.light + beam), air), radial * select(0.0, 1.0, particle.u32_1 < wp.visible_count));
"#);
        context.fragment_code.push_str(if self.snow {
            "let snow = true;\n"
        } else {
            "let snow = false;\n"
        });
        context.fragment_code.push_str(include_str!(
            "../../openrailsrs-bevy-scenery/assets/shaders/hanabi_weather_fragment.wgsl"
        ));
        Ok(())
    }
}

fn weather_asset(snow: bool, count: u32) -> EffectAsset {
    let writer = ExprWriter::new();
    for (name, value) in [
        ("center", Vec4::ZERO),
        ("phase", Vec4::ZERO),
        ("right", Vec4::ZERO),
        ("up", Vec4::ZERO),
        ("wind_time", Vec4::ZERO),
        ("grid", Vec4::ZERO),
        ("fog", Vec4::ZERO),
        ("lamp_position", Vec4::ZERO),
        ("lamp_direction", Vec4::ZERO),
    ] {
        writer.add_property(name, value.into());
    }
    writer.add_property("shelter_base", 0.0_f32.into());
    writer.add_property("light", 1.0_f32.into());
    writer.add_property("visible_count", count.into());
    let mut module = writer.finish();
    module.add_texture_slot("shelter");
    EffectAsset::new(count, SpawnerSettings::rate(0.0.into()), module)
        .with_name(if snow {
            "native weather snow"
        } else {
            "native weather rain"
        })
        .with_simulation_space(SimulationSpace::Global)
        .with_simulation_condition(SimulationCondition::Always)
        .with_motion_integration(MotionIntegration::None)
        .init(WeatherMotion { init: true, snow })
        .update(WeatherMotion { init: false, snow })
        .render(WeatherRender { snow })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precipitation_generates_persistent_seed_motion_and_roof_mask() {
        for snow in [false, true] {
            let asset = weather_asset(snow, 256);
            bevy_hanabi::EffectShaderSources::generate(&asset, None, 0).unwrap();
            assert_eq!(asset.capacity(), 256);
            assert_eq!(asset.motion_integration, MotionIntegration::None);
        }
    }
}
