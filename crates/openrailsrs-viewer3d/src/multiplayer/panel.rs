use super::*;
use bevy::input::{
    ButtonState,
    keyboard::{Key, KeyboardInput},
    mouse::{MouseScrollUnit, MouseWheel},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Address,
    Identity,
    Room,
    Switch,
    Signal,
}
#[derive(Clone, Component)]
pub(super) enum Action {
    Close,
    Field(Field),
    Host,
    Join,
    Disconnect,
    Save,
    Restore,
    Reveal,
    CopyRoom,
    PasteRoom,
    ResumeRegulators,
    Role(Role),
    Train(String),
    Follow(String),
    Drive(u8, f64),
    Direction(f64),
    Light(u8),
    Toggle(u8),
    Fire(u8, f64),
    Steam(f64),
    Camera(ObserverCamera),
    Switch(bool),
    Signal(u8),
}
#[derive(Component)]
pub(super) struct Scroll;
const TEXT: Color = Color::srgb(0.92, 0.95, 0.98);
fn label(p: &mut ChildSpawnerCommands, value: impl Into<String>, size: f32) {
    p.spawn((
        Text::new(value),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(TEXT),
        Node {
            flex_shrink: 0.,
            ..default()
        },
    ));
}
fn button(p: &mut ChildSpawnerCommands, value: impl Into<String>, action: Action) {
    p.spawn((
        Button,
        action,
        Node {
            padding: UiRect::axes(Val::Px(9.), Val::Px(6.)),
            min_height: Val::Px(30.),
            margin: UiRect::all(Val::Px(2.)),
            flex_shrink: 0.,
            ..default()
        },
        BackgroundColor(Color::srgb(0.11, 0.19, 0.26)),
    ))
    .with_children(|p| label(p, value, 13.));
}
fn row(p: &mut ChildSpawnerCommands, build: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn(Node {
        display: Display::Flex,
        flex_wrap: FlexWrap::Wrap,
        align_items: AlignItems::Center,
        ..default()
    })
    .with_children(build);
}
pub fn rebuild(mut commands: Commands, mut state: ResMut<MultiplayerState>) {
    if !state.render_ui || state.ui_revision == state.revision {
        return;
    }
    state.ui_revision = state.revision;
    if let Some(root) = state.root.take() {
        commands.entity(root).despawn();
    }
    if !state.open {
        return;
    }
    let root=commands.spawn((crate::player_ui::ScreenHud,Node{position_type:PositionType::Absolute,
        left:Val::Percent(3.),top:Val::Percent(4.),width:Val::Percent(94.),height:Val::Percent(90.),
        flex_direction:FlexDirection::Column,padding:UiRect::all(Val::Px(14.)),overflow:Overflow::clip(),..default()},
        GlobalZIndex(90),BackgroundColor(Color::srgb(0.025,0.045,0.065))))
        .with_children(|p|{
            row(p,|p|{label(p,"Multijugador · sesión compartida",22.);button(p,"Volver · Esc",Action::Close);});
            label(p,format!("{} · {} · {} cuadros · {}",if state.host(){"Servidor"}else if state.active(){"Cliente"}else{"Sin conexión"},state.role.label(),state.frames,
                state.rtt_ms.map(|v|format!("RTT {v:.0} ms")).unwrap_or_default()),13.);
            label(p,state.notice.clone(),13.);
            p.spawn((Scroll,ScrollPosition(Vec2::new(0.,state.scroll_y)),Node{flex_grow:1.,min_height:Val::Px(0.),overflow:Overflow::scroll_y(),flex_direction:FlexDirection::Column,..default()}))
                .with_children(|p|{
                    if !state.active(){
                        label(p,"Abrí el mismo servicio instalado en todos los equipos y elegí Host o Conectar. Dirección inicial local:127.0.0.1.",13.);
                        button(p,format!("Servidor: {}",state.form.address),Action::Field(Field::Address));
                        button(p,format!("Tu nombre: {}",state.form.identity),Action::Field(Field::Identity));
                        row(p,|p|{button(p,format!("Clave de sala: {}",if state.form.show_room{state.form.room.0.clone()}else{"•".repeat(state.form.room.0.chars().count().min(32))}),Action::Field(Field::Room));button(p,"Mostrar/ocultar",Action::Reveal);button(p,"Pegar clave",Action::PasteRoom);});
                        label(p,"El host puede dejar la clave vacía para generar una. Para unirte necesitás la clave y un nombre diferente; la identidad de reconexión queda privada en este equipo.",12.);
                    }
                    row(p,|p|for role in [Role::Driver,Role::Fireman,Role::Dispatcher,Role::Spectator]{button(p,format!("{}{}",if (if state.active(){state.role}else{state.form.role})==role{"✓ "}else{""},role.label()),Action::Role(role));});
                    label(p,"Servicios y puestos",16.);
                    for train in &state.catalog{
                        row(p,|p|{button(p,format!("{} · {} coches{}",train.id,train.cars,if train.steam{" · vapor"}else{""}),Action::Train(train.id.clone()));
                            if state.active(){button(p,"Seguir",Action::Follow(train.id.clone()));}});
                    }
                    if !state.active(){row(p,|p|{button(p,"Abrir sala (Host)",Action::Host);button(p,"Conectar",Action::Join);button(p,"Recuperar sesión del host",Action::Restore);});}
                    else{row(p,|p|{button(p,"Desconectar",Action::Disconnect);if state.host(){button(p,"Guardar sesión completa",Action::Save);}});}
                    if state.host(){
                        label(p,format!("Servidor: {}",state.form.address),13.);
                        row(p,|p|{label(p,format!("Clave de sala: {}",if state.form.show_room{state.form.room.0.clone()}else{"•".repeat(16)}),13.);button(p,"Mostrar/ocultar clave",Action::Reveal);button(p,"Copiar clave",Action::CopyRoom);});
                    }
                    if state.active(){
                        label(p,"Conexiones · un puesto permanece reservado durante una desconexión",15.);
                        for member in &state.roster{label(p,format!("{} · {} · {} · {}",member.identity,member.role.label(),member.train.as_deref().unwrap_or("observación/despacho"),if member.connected{"conectado"}else{"desconectado"}),12.);}
                        if let Some(view)=state.selected_view(){
                            label(p,format!("{} · {:.1}km/h · {:.1}s",view.id,view.velocity_mps*3.6,view.time_s),16.);
                            if state.role==Role::Driver{
                                label(p,format!("Regulador {:.0}% · freno {:.0}% · inversor {}",view.driver.throttle*100.,view.driver.train_brake*100.,if view.driver.direction==0.5{"Neutro"}else if view.driver.direction>0.5{"Adelante"}else{"Atrás"}),13.);
                                row(p,|p|{button(p,"Regulador−",Action::Drive(0,-0.1));button(p,"Regulador+",Action::Drive(0,0.1));button(p,"Freno−",Action::Drive(1,-0.15));button(p,"Freno+",Action::Drive(1,0.15));
                                    if view.driver.independent_brake.is_some(){button(p,"Independiente−",Action::Drive(2,-0.1));button(p,"Independiente+",Action::Drive(2,0.1));}});
                                row(p,|p|{button(p,"Atrás",Action::Direction(0.));button(p,"Neutro",Action::Direction(0.5));button(p,"Adelante",Action::Direction(1.));button(p,"Bocina",Action::Toggle(0));button(p,"Limpiaparabrisas",Action::Toggle(1));button(p,"Luz cabina",Action::Toggle(2));});
                                row(p,|p|for n in 0..=2{button(p,format!("Faros {n}"),Action::Light(n));});
                                if view.fireman.is_some(){row(p,|p|{button(p,"Corte−",Action::Steam(-0.05));button(p,"Corte+",Action::Steam(0.05));button(p,"Grifos cilindros",Action::Toggle(3));});}
                                if view.has_local_regulators{button(p,"Recuperar reguladores · mando del conductor",Action::ResumeRegulators);}
                                label(p,"Fuera de este panel se usan tus teclas de regulador, frenos, inversor, bocina, luces y limpiaparabrisas configuradas.",12.);
                            }
                            if let Some(fire)=&view.fireman{
                                label(p,format!("Caldera {:.2}bar · agua {:.0}kg · fuego {:.0}% · tiro {:.0}% · fogonero {}",view.boiler_pressure_bar.unwrap_or(0.),view.boiler_water_kg.unwrap_or(0.),fire.firing*100.,fire.damper*100.,if fire.automatic{"automático"}else{"manual"}),13.);
                                if state.role==Role::Fireman{
                                    row(p,|p|{button(p,"Fuego−",Action::Fire(0,-0.05));button(p,"Fuego+",Action::Fire(0,0.05));button(p,"Tiro−",Action::Fire(1,-0.05));button(p,"Tiro+",Action::Fire(1,0.05));});
                                    row(p,|p|{button(p,"Inyector1",Action::Fire(2,0.));button(p,"Inyector2",Action::Fire(3,0.));button(p,"Soplador",Action::Fire(4,0.));button(p,"Automático/manual",Action::Fire(5,0.));});
                                }
                            }
                        }
                        if state.role==Role::Dispatcher{
                            label(p,"Despacho · se respetan ocupación, enclavamientos y reservas",15.);
                            button(p,format!("Cambio: {}",state.dispatch_switch),Action::Field(Field::Switch));
                            row(p,|p|{button(p,"Recta",Action::Switch(false));button(p,"Desviada",Action::Switch(true));});
                            button(p,format!("Señal: {}",state.dispatch_signal),Action::Field(Field::Signal));
                            row(p,|p|{button(p,"Roja",Action::Signal(0));button(p,"Precaución",Action::Signal(1));button(p,"Libre",Action::Signal(2));button(p,"Automática",Action::Signal(3));});
                            label(p,"Los identificadores son los del mapa M. Las órdenes se introducen aquí; el mapa local queda de consulta.",12.);
                        }
                        label(p,"Cámara de seguimiento",15.);row(p,|p|{button(p,"Exterior",Action::Camera(ObserverCamera::Exterior));button(p,"Orbitar",Action::Camera(ObserverCamera::Orbit));button(p,"Libre",Action::Camera(ObserverCamera::Free));});
                        label(p,"Espectador: sólo observación, sin autoridad de conducción. F9/B/patio y órdenes del mapa local usan las herramientas del anfitrión fuera de esta sesión.",12.);
                    }
                });
        }).id();
    state.root = Some(root);
}
pub fn keyboard(
    mut events: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    settings: Option<Res<crate::player_settings::PlayerSettings>>,
    ui: Option<Res<crate::player_ui::PlayerUiState>>,
    mut state: ResMut<MultiplayerState>,
) {
    let state = &mut *state;
    if state.open {
        for input in events.read() {
            if input.state != ButtonState::Pressed {
                continue;
            }
            if input.logical_key == Key::Escape {
                state.open = false;
                state.focused = None;
                state.dirty();
                continue;
            }
            let Some(field) = state.focused else {
                continue;
            };
            if state.active() && matches!(field, Field::Address | Field::Identity | Field::Room) {
                continue;
            }
            let value = match field {
                Field::Address => &mut state.form.address,
                Field::Identity => &mut state.form.identity,
                Field::Room => &mut state.form.room.0,
                Field::Switch => &mut state.dispatch_switch,
                Field::Signal => &mut state.dispatch_signal,
            };
            if input.logical_key == Key::Backspace {
                value.pop();
            } else if input.logical_key == Key::Enter {
                state.focused = None;
            } else if let Some(text) = &input.text {
                for c in text.chars().filter(|c| !c.is_control()) {
                    if value.len() + c.len_utf8() <= 128 {
                        value.push(c);
                    }
                }
            }
            state.dirty();
        }
        return;
    }
    for _ in events.read() {}
    if !state.active()
        || !state.connected
        || state.role != Role::Driver
        || ui.is_some_and(|ui| ui.panel != crate::player_ui::PlayerPanel::None)
    {
        return;
    }
    let Some(settings) = settings else {
        return;
    };
    use crate::player_settings::PlayerAction as A;
    let Some(mut driver) = state.driver_draft() else {
        return;
    };
    let mut changed = false;
    for (action, field, delta) in [
        (A::ThrottleUp, 0, 0.1),
        (A::ThrottleDown, 0, -0.1),
        (A::BrakeUp, 1, 0.15),
        (A::BrakeDown, 1, -0.15),
        (A::IndependentUp, 2, 0.1),
        (A::IndependentDown, 2, -0.1),
    ] {
        if settings.just_pressed(&keys, action) {
            match field {
                0 => driver.throttle = (driver.throttle + delta).clamp(0., 1.),
                1 => driver.train_brake = (driver.train_brake + delta).clamp(0., 1.),
                _ => {
                    if let Some(value) = &mut driver.independent_brake {
                        *value = (*value + delta).clamp(0., 1.);
                    }
                }
            }
            changed = true;
        }
    }
    if settings.just_pressed(&keys, A::Forward) {
        driver.direction = (driver.direction + 0.5).min(1.);
        changed = true;
    }
    if settings.just_pressed(&keys, A::Reverse) {
        driver.direction = (driver.direction - 0.5).max(0.);
        changed = true;
    }
    if settings.just_pressed(&keys, A::Neutral) {
        driver.direction = 0.5;
        changed = true;
    }
    if settings.just_pressed(&keys, A::Headlights) {
        driver.headlights = (driver.headlights + 1) % 3;
        changed = true;
    }
    if settings.just_pressed(&keys, A::CabLight) {
        driver.cab_light = !driver.cab_light;
        changed = true;
    }
    if settings.just_pressed(&keys, A::Wiper) {
        driver.wipers = !driver.wipers;
        changed = true;
    }
    if keys.just_pressed(settings.key(A::Horn)) || keys.just_released(settings.key(A::Horn)) {
        driver.horn = keys.pressed(settings.key(A::Horn));
        changed = true;
    }
    if settings.just_pressed(&keys, A::Emergency) {
        driver.throttle = 0.;
        driver.train_brake = 1.;
        changed = true;
    }
    if changed {
        state.queue(RemoteCommand::Drive { controls: driver });
    }
}
pub fn buttons(
    interactions: Query<(&Interaction, &Action), Changed<Interaction>>,
    mut state: ResMut<MultiplayerState>,
    mut wheel: MessageReader<MouseWheel>,
    mut scroll: Query<&mut ScrollPosition, With<Scroll>>,
    mut clipboard: Option<ResMut<bevy_clipboard::Clipboard>>,
) {
    let state = &mut *state;
    for event in wheel.read() {
        if state.open {
            for mut position in &mut scroll {
                position.y = (position.y
                    - event.y
                        * if event.unit == MouseScrollUnit::Line {
                            28.
                        } else {
                            1.
                        })
                .max(0.);
                state.scroll_y = position.y;
            }
        }
    }
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            Action::Close => {
                state.open = false;
                state.focused = None;
            }
            Action::Field(field) => state.focused = Some(*field),
            Action::Host => state.actions.push(NetworkAction::Host),
            Action::Join => state.actions.push(NetworkAction::Join),
            Action::Disconnect => state.actions.push(NetworkAction::Disconnect),
            Action::Save => state.actions.push(NetworkAction::Save),
            Action::Restore => state.actions.push(NetworkAction::Restore),
            Action::Reveal => state.form.show_room = !state.form.show_room,
            Action::CopyRoom => {
                state.notice = match clipboard
                    .as_mut()
                    .and_then(|c| c.set_text(state.form.room.0.clone()).ok())
                {
                    Some(()) => "Clave copiada al portapapeles".into(),
                    None => "Portapapeles no disponible; podés mostrar la clave".into(),
                };
            }
            Action::PasteRoom => {
                if !state.active() {
                    if let Some(Ok(value)) = clipboard
                        .as_mut()
                        .and_then(|c| c.fetch_text().poll_result())
                    {
                        let secret = Secret(value.trim().into());
                        if secret.valid() {
                            state.form.room = secret;
                        } else {
                            state.notice="La clave pegada debe tener entre 12 y 128 bytes, sin caracteres de control".into();
                        }
                    } else {
                        state.notice = "No se pudo leer el portapapeles".into();
                    }
                }
            }
            Action::Role(role) => {
                state.form.role = *role;
                if state.active() {
                    let train = if role.needs_train() {
                        state
                            .form
                            .train
                            .clone()
                            .or(state.train.clone())
                            .or_else(|| state.catalog.first().map(|t| t.id.clone()))
                    } else {
                        None
                    };
                    state.queue(RemoteCommand::Role { role: *role, train });
                }
            }
            Action::Train(id) => {
                state.form.train = Some(id.clone());
                if state.active() {
                    if state.role.needs_train() {
                        let role = state.role;
                        state.queue(RemoteCommand::Role {
                            role,
                            train: Some(id.clone()),
                        });
                    } else {
                        state.queue(RemoteCommand::Follow { train: id.clone() });
                    }
                }
            }
            Action::Follow(id) => {
                state.queue(RemoteCommand::Follow { train: id.clone() });
                state.followed = Some(id.clone());
            }
            Action::Camera(camera) => state.camera = *camera,
            Action::ResumeRegulators => state.queue(RemoteCommand::ResumeRegulators),
            Action::Switch(diverging) => {
                let node = state.dispatch_switch.clone();
                state.queue(RemoteCommand::Switch {
                    node,
                    position: if *diverging {
                        openrailsrs_track::SwitchPosition::Diverging
                    } else {
                        openrailsrs_track::SwitchPosition::Straight
                    },
                });
            }
            Action::Signal(value) => {
                let id = state.dispatch_signal.clone();
                let aspect = match value {
                    0 => Some(openrailsrs_track::SignalAspect::Stop),
                    1 => Some(openrailsrs_track::SignalAspect::Caution),
                    2 => Some(openrailsrs_track::SignalAspect::Clear),
                    _ => None,
                };
                state.queue(RemoteCommand::Signal { id, aspect });
            }
            action => {
                let Some(mut driver) = state.driver_draft() else {
                    continue;
                };
                let fire = state.fireman_draft();
                match action {
                    Action::Drive(field, delta) => {
                        match field {
                            0 => driver.throttle = (driver.throttle + delta).clamp(0., 1.),
                            1 => driver.train_brake = (driver.train_brake + delta).clamp(0., 1.),
                            _ => {
                                if let Some(value) = &mut driver.independent_brake {
                                    *value = (*value + delta).clamp(0., 1.);
                                }
                            }
                        }
                        state.queue(RemoteCommand::Drive { controls: driver });
                    }
                    Action::Direction(direction) => {
                        driver.direction = *direction;
                        state.queue(RemoteCommand::Drive { controls: driver });
                    }
                    Action::Light(value) => {
                        driver.headlights = *value;
                        state.queue(RemoteCommand::Drive { controls: driver });
                    }
                    Action::Toggle(value) => {
                        match value {
                            0 => driver.horn = !driver.horn,
                            1 => driver.wipers = !driver.wipers,
                            2 => driver.cab_light = !driver.cab_light,
                            _ => {
                                let cutoff = state.steam_cutoff;
                                let cylinder_cocks = !state.cylinder_cocks;
                                state.queue(RemoteCommand::SteamDriver {
                                    cutoff,
                                    cylinder_cocks,
                                });
                                continue;
                            }
                        }
                        state.queue(RemoteCommand::Drive { controls: driver });
                    }
                    Action::Steam(delta) => {
                        let cutoff = (state.steam_cutoff + delta).clamp(0., 0.75);
                        let cocks = state.cylinder_cocks;
                        state.queue(RemoteCommand::SteamDriver {
                            cutoff,
                            cylinder_cocks: cocks,
                        });
                    }
                    Action::Fire(field, delta) => {
                        if let Some(mut controls) = fire {
                            match field {
                                0 => {
                                    controls.firing = (controls.firing + delta).clamp(0., 1.);
                                    controls.automatic = false;
                                }
                                1 => {
                                    controls.damper = (controls.damper + delta).clamp(0., 1.);
                                    controls.automatic = false;
                                }
                                2 => controls.injector1 = !controls.injector1,
                                3 => controls.injector2 = !controls.injector2,
                                4 => controls.blower = !controls.blower,
                                _ => controls.automatic = !controls.automatic,
                            }
                            state.queue(RemoteCommand::Fire { controls });
                        }
                    }
                    _ => {}
                }
            }
        }
        state.dirty();
    }
}
