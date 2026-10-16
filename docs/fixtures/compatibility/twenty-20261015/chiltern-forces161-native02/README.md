# Chiltern original all-car diagnostics02

This is an additive observation of the original OR1.6.1 Chiltern service, in its historical paused context. The original20column CSV retains all15003rows and SHA256 `ad9e690435d784df243c8a49bfdb37137c6e5f98f8941045adfde27e4500a7e2`. The IIR observation is also byte-identical to observation01; neither historical reference was replaced.

The lossless CSV contains120024car rows, with direct field reads after every Simulator.Update and one initial sample. CurveForceN is the raw body resistance actually used by Train; CurveForceNFiltered is a separate smoothed audio value. Wind and tunnel forces are zero throughout this captured service. Stored brake friction coefficient fields absent from the DLL are empty, never recalculated by an observer getter.

Force sampleN uses radius and physical cant from sampleN-1. The current locomotive coefficient is published in car order: head for cars0..6, rear for car7. Its speed comes from that locomotive's previous AbsSpeedMpS cache. Source-phase diagnosis reproduces120016original force fields exactly using Single operations, but this Python calculation is a diagnosis, not an oracle or an executed Rust test. Expected forces in the sparse fixture are exclusively direct DLL fields.

The sparse fixture has256calls at32post-update ticks, all8cars, including zero-radius boundaries, 250m curves and the native maximum train curve resistance6482.75N. The separate protocol declares0.01N absolute force error before the first Rust comparison; existing service motion tolerances remain unchanged. The coefficient input is explicitly source-derived from independently observed environmental base and prior speed; the final rear train coefficient is directly observed as an additional cross-check.

Only the isolated subsystem test may prescribe these mass/speed/geometry/coefficient inputs. Full trajectories independently import authored TDB/TSECTION geometry and evolve their own positions and forces. No force, radius, cant, speed, pose, pressure, filter output or wheel state recorded here is an input to a closed trajectory.

Original ENG/WAG/CON/ACT/SRV/PAT/TDB/TRK/DLL assets, the Wine runtime, six human trace contents and duplicate IIR/environment inputs are excluded. `receipt.json` and the isolation/release receipts preserve the actual capture metadata and an empty private-process release. Publication01, canonical environmental CSV and human trace hashes remain unchanged. No Rust execution or workspace gate result is claimed by this publication.
