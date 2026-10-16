//! Exercises production readers. Invalid input is expected; panics are failures.
//! No files, network, GPU, caches or user content are accessed by these targets.
use openrailsrs_formats as formats;
use std::path::Path;

pub const MAX_INPUT: usize = 65_536;
pub const TARGETS: [&str; 7] = [
    "stf",
    "simisa",
    "textures",
    "shapes",
    "terrain",
    "sigscript",
    "multiplayer",
];

pub fn exercise(target: &str, bytes: &[u8]) {
    if bytes.len() > MAX_INPUT {
        return;
    }
    let text = String::from_utf8_lossy(bytes);
    match target {
        "stf" => {
            let _ = formats::parse(&text);
            let _ = formats::parse_named_stf(&text);
            let _ = formats::parse_vehicle_text(&text);
            let _ = formats::parse_cab_view_text(&text);
            let _ = formats::parse_activity_text(&text);
        }
        "simisa" => {
            if let Ok(payload) =
                formats::msts_simisa::decode_simisa_container_limited(bytes, MAX_INPUT)
            {
                assert!(payload.data_offset <= payload.bytes.len());
                assert!(payload.bytes.len() <= MAX_INPUT);
            }
            let _ = formats::decode_msts_bytes(bytes);
        }
        "textures" => {
            let _ = openrailsrs_ace::validate_dds(bytes, 1024 * 1024);
            if let Ok(ace) = openrailsrs_ace::AceFile::read_bytes_limited(bytes, 1024 * 1024) {
                assert_eq!(ace.mip0.len(), ace.width as usize * ace.height as usize * 4);
                if let Ok(dds) = ace.to_dds() {
                    let info = openrailsrs_ace::validate_dds(&dds, 2 * 1024 * 1024)
                        .expect("valid ACE exports valid DDS");
                    assert_eq!((info.width, info.height), (ace.width, ace.height));
                }
            }
        }
        "shapes" => {
            if let Ok(payload) =
                formats::msts_simisa::decode_simisa_container_limited(bytes, MAX_INPUT)
                && !payload.is_text
            {
                let _ = formats::binary_shape_to_ast(&payload);
                if let Ok(shape) = formats::shape_from_binary_payload(&payload) {
                    let _ = formats::validate_shape_trilist_topology(&shape);
                }
            }
            let mut reader = formats::BinaryBlockReader::new(bytes, 0, usize::MAX);
            for _ in 0..8 {
                let Ok(mut child) = reader.open_sub_block() else {
                    break;
                };
                let _ = child.read_u32();
                let _ = child.read_f32();
                let _ = child.read_string_utf16();
            }
        }
        "terrain" => {
            let word = |offset: usize| {
                let mut n = [0; 4];
                for (i, b) in bytes.iter().skip(offset).take(4).enumerate() {
                    n[i] = *b;
                }
                i32::from_le_bytes(n)
            };
            let (x, z) = (word(0), word(4));
            let filename = formats::world_w_filename_from_tile_xz(x, z);
            let parsed = formats::parse_world_w_tile_xz(Path::new(&filename));
            if x.unsigned_abs() <= 999_999 && z.unsigned_abs() <= 999_999 {
                assert_eq!(parsed, Some((x, z)));
            }
            let _ = formats::msts_tile_name::msts_tile_name_from_xz_zoom(x, z, word(8) as u32);
            let n = word(12) as u32;
            let raw = bytes.get(16..).unwrap_or_default();
            let params = formats::TerrainSamples {
                nsamples: n,
                ..Default::default()
            };
            if let Ok(grid) = formats::read_y_raw_bytes(raw, &params) {
                assert!(grid.nsamples > 0);
                assert!(grid.elevations.iter().all(|h| h.is_finite()));
            }
            if let Ok(grid) = formats::read_f_raw_bytes(raw.to_vec(), &params) {
                assert!(grid.nsamples > 0);
            }
            let _ = formats::parse_world_w_tile_xz(Path::new(text.as_ref()));
        }
        "sigscript" => {
            if let Ok(program) = openrailsrs_track::sigscript::SignalProgram::compile(&text) {
                for enabled in [false, true] {
                    let context = openrailsrs_track::sigscript::SignalContext {
                        enabled,
                        block_clear: enabled,
                        ..Default::default()
                    };
                    if let Ok(result) = program.evaluate(context) {
                        assert!(result.aspect < 8);
                        assert!((-1..=255).contains(&result.draw_state));
                    }
                }
            }
        }
        "multiplayer" => {
            use openrailsrs_game::multiplayer::{
                ClientMessage, MAX_REQUEST_BYTES,
                transport::{decode_frame, encode},
            };
            let mut accumulated = Vec::new();
            for chunk in bytes.chunks(1 + bytes.first().copied().unwrap_or(0) as usize) {
                accumulated.extend_from_slice(chunk);
                for _ in 0..8 {
                    match decode_frame::<ClientMessage>(&accumulated, MAX_REQUEST_BYTES) {
                        Ok(Some((message, n))) => {
                            assert!(n <= accumulated.len());
                            if let Ok(encoded) = encode(&message, MAX_REQUEST_BYTES) {
                                let (roundtrip, _) =
                                    decode_frame::<ClientMessage>(&encoded, MAX_REQUEST_BYTES)
                                        .unwrap()
                                        .unwrap();
                                assert_eq!(
                                    serde_json::to_value(&message).unwrap(),
                                    serde_json::to_value(&roundtrip).unwrap()
                                );
                            }
                            if let ClientMessage::Command {
                                action, sequence, ..
                            } = message
                            {
                                use openrailsrs_game::multiplayer::{
                                    Authority, Hello, PROTOCOL_VERSION, Role, Secret,
                                    ServerMessage, TrainInfo,
                                };
                                let mut authority = Authority::new(
                                    Secret("owned-fuzz-room".into()),
                                    "owned-content".into(),
                                )
                                .unwrap();
                                let trains = [TrainInfo {
                                    id: "owned-train".into(),
                                    steam: false,
                                    cars: 1,
                                    departed: false,
                                }];
                                let welcome = authority
                                    .connect(
                                        1,
                                        Hello {
                                            version: PROTOCOL_VERSION,
                                            room: Secret("owned-fuzz-room".into()),
                                            identity: "owned-observer".into(),
                                            credential: Secret("owned-fuzz-credential".into()),
                                            content_id: "owned-content".into(),
                                            role: Role::Spectator,
                                            train: None,
                                        },
                                        &trains,
                                    )
                                    .unwrap();
                                let ServerMessage::Welcome { epoch, lease, .. } = welcome else {
                                    unreachable!()
                                };
                                // One arbitrary command cannot grant an observer authority
                                // to call physical railway operations, even at u64 limits.
                                let _ = authority.command(
                                    1,
                                    epoch,
                                    lease,
                                    sequence,
                                    action,
                                    &trains,
                                    |_, _| panic!("observer reached railway callback"),
                                );
                            }
                            accumulated.drain(..n);
                        }
                        Ok(None) => break,
                        Err(_) => return,
                    }
                }
            }
        }
        _ => panic!("unknown fuzz target"),
    }
}

fn block(token: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = token.to_le_bytes().to_vec();
    bytes.extend(0u16.to_le_bytes());
    bytes.extend((body.len() as u32 + 1).to_le_bytes());
    bytes.push(0);
    bytes.extend(body);
    bytes
}

pub fn seeds(target: &str) -> Vec<Vec<u8>> {
    match target {
        "stf" => vec![
            b"Wagon ( Coach Mass ( 40t ) Size ( 3m 4m 20m ) )".to_vec(),
            b"(A (B 2) \"Unicode caf\xc3\xa9\")".to_vec(),
            format!("{}0{}", "(".repeat(160), ")".repeat(160)).into_bytes(),
        ],
        "simisa" => vec![
            b"SIMISA@@@@@@@@@@JINX0s1t______\r\nShape ( )".to_vec(),
            vec![0xff, 0xfe, 0x41, 0, 0x42],
            // Own zlib-compressed text with a deliberately false declared size.
            // Keep a valid stream and a truncated stream in every CI corpus.
            vec![
                83, 73, 77, 73, 83, 65, 64, 70, 0, 148, 53, 119, 64, 64, 64, 64, 120, 156, 243,
                242, 244, 139, 48, 40, 54, 44, 137, 7, 3, 94, 174, 224, 140, 196, 130, 84, 5, 13,
                5, 77, 0, 101, 124, 7, 85,
            ],
            b"SIMISA@F@@@@@@@@\x78\x9c\xf3\xf2\xf4\x8b\x30\x28\x36\x2c".to_vec(),
        ],
        "textures" => {
            let mut ace = b"@ACE".to_vec();
            for n in [4u32, 4, 0] {
                ace.extend(n.to_le_bytes());
            }
            ace.extend([1, 4, 0, 0]);
            ace.extend([37u8; 64]);
            let dds = openrailsrs_ace::AceFile::read_bytes(&ace)
                .unwrap()
                .to_dds()
                .unwrap();
            let mut missing_green = b"SIMISA@@@@@@@@@@".to_vec();
            let mut header = vec![0; 152];
            for (at, value) in [(0, 1u32), (8, 4), (12, 4), (20, 1)] {
                header[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            missing_green.extend(header);
            missing_green.extend(8u64.to_le_bytes());
            missing_green.extend(3u64.to_le_bytes());
            missing_green.extend([0; 32]);
            vec![ace, dds, missing_green]
        }
        "shapes" => {
            let mut shape = b"SIMISA@@@@@@@@@@JINX0s1b_____\r\n\r".to_vec();
            shape.extend(71u16.to_le_bytes());
            shape.extend(0u16.to_le_bytes());
            shape.extend(1u32.to_le_bytes());
            shape.push(0);
            let mut points = 1u32.to_le_bytes().to_vec();
            let coordinates = [0f32, 1., 2.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>();
            points.extend(block(2, &coordinates));
            let root = block(71, &block(7, &points));
            let mut rich = b"SIMISA@@@@@@@@@@JINX0s1b_____\r\n\r".to_vec();
            rich.extend(&root);
            vec![shape, rich, root]
        }
        "terrain" => {
            let mut header = vec![0; 12];
            header.extend(2u32.to_le_bytes());
            let mut heights = header.clone();
            heights.extend([0, 0, 1, 0, 2, 0, 3, 0]);
            let mut flags = header;
            flags.extend([0, 4, 1, 0]);
            vec![
                vec![0; 8],
                i32::MIN.to_le_bytes().repeat(4),
                b"w-006084+014923.w".to_vec(),
                heights,
                flags,
            ]
        }
        "sigscript" => vec![
            b"STATE = SIGASP_STOP; DRAW_STATE = 0;".to_vec(),
            b"if (ENABLED) { STATE=7; } else { STATE=0; }".to_vec(),
        ],
        "multiplayer" => {
            use openrailsrs_game::multiplayer::{
                ClientMessage, DriverControls, MAX_REQUEST_BYTES, RemoteCommand, transport::encode,
            };
            vec![
                encode(
                    &ClientMessage::Command {
                        epoch: 1,
                        lease: 1,
                        sequence: u64::MAX,
                        action: RemoteCommand::Ping { client_millis: 0 },
                    },
                    MAX_REQUEST_BYTES,
                )
                .unwrap(),
                vec![0xff; 4],
                encode(
                    &ClientMessage::Command {
                        epoch: 1,
                        lease: 1,
                        sequence: 1,
                        action: RemoteCommand::Drive {
                            controls: DriverControls {
                                throttle: 1.,
                                train_brake: 0.,
                                independent_brake: None,
                                direction: 1.,
                                headlights: 0,
                                cab_light: false,
                                wipers: false,
                                horn: false,
                            },
                        },
                    },
                    MAX_REQUEST_BYTES,
                )
                .unwrap(),
            ]
        }
        _ => panic!("unknown fuzz target"),
    }
}
