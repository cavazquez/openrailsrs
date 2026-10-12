use super::*;
pub(super) fn build(
    p: &mut ChildSpawnerCommands<'_>,
    campaigns: &crate::campaign_play::CampaignPlay,
) {
    label(
        p,
        "Elegí una campaña. Cada jornada conserva resultados y, si usa la misma formación, combustible, agua, averías y estado del equipo.",
        14.,
        TEXT,
    );
    for (i, c) in campaigns.entries.iter().enumerate() {
        button(
            p,
            format!(
                "{} {} · v{}",
                if i == campaigns.selected {
                    "●"
                } else {
                    "○"
                },
                c.definition.campaign.name,
                c.definition.campaign.version
            ),
            UiCommand::CampaignSelect(i),
        );
    }
    let Some(c) = campaigns.entries.get(campaigns.selected) else {
        label(
            p,
            "No se encontraron campañas. Las definiciones propias van en la carpeta de datos/campaign-definitions; los ejemplos incluyen una campaña corta de prueba.",
            13.,
            MUTED,
        );
        return;
    };
    label(p, &c.definition.campaign.description, 13., MUTED);
    for (i, status) in
        openrailsrs_campaign::mission_statuses(&c.definition, &campaigns.progress.results)
            .iter()
            .enumerate()
    {
        let state = match status.state {
            openrailsrs_campaign::MissionState::Locked => "Requiere jornadas previas",
            openrailsrs_campaign::MissionState::Available => "Disponible",
            openrailsrs_campaign::MissionState::Completed => "Completada",
        };
        label(
            p,
            format!(
                "Jornada {} · {} · {}{}",
                i + 1,
                status.def.name,
                state,
                status
                    .best_score
                    .map_or(String::new(), |s| format!(" · mejor {s}/100"))
            ),
            16.,
            if status.state == openrailsrs_campaign::MissionState::Locked {
                MUTED
            } else {
                TEXT
            },
        );
        label(p, &status.def.description, 12., MUTED);
        if status.state != openrailsrs_campaign::MissionState::Locked {
            button(p, "Conducir esta jornada", UiCommand::CampaignStart(i));
        }
    }
    label(p, &campaigns.status, 13., ACCENT);
    if let Some(id) = &campaigns.progress.last_mission {
        label(
            p,
            format!(
                "Última jornada: {id}. El progreso se guarda en tu carpeta de datos, sin modificar los escenarios."
            ),
            12.,
            MUTED,
        );
    }
}
