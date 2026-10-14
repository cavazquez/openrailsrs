// Calls the unmodified, pinned OR 1.6.1 assembly. No coupling implementation
// or substitute equation lives here. Traveller positions and subsystem inputs
// are prescribed QA inputs; this is not a driven-route or impact-physics test.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Serialization;
using System.Threading;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using Microsoft.Xna.Framework;
using ORTS.Common;
using ORTS.Settings;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;

internal static class CouplingReference
{
    static readonly CultureInfo Culture = CultureInfo.InvariantCulture;
    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static object Field(object obj, string name) {
        for (Type t = obj.GetType(); t != null; t = t.BaseType) {
            var field = t.GetField(name, BindingFlags.Instance | BindingFlags.Public |
                BindingFlags.NonPublic | BindingFlags.DeclaredOnly);
            if (field != null) return field.GetValue(obj);
        }
        throw new MissingMemberException(obj.GetType().Name, name);
    }
    static void SetField(object obj, string name, object value) {
        for (Type t = obj.GetType(); t != null; t = t.BaseType) {
            var field = t.GetField(name, BindingFlags.Instance | BindingFlags.Public |
                BindingFlags.NonPublic | BindingFlags.DeclaredOnly);
            if (field != null) { field.SetValue(obj, value); return; }
        }
        throw new MissingMemberException(obj.GetType().Name, name);
    }
    static object Point(Traveller traveller) {
        var w = traveller.WorldLocation;
        return new { tile_x = w.TileX, tile_z = w.TileZ, x = w.Location.X,
            y = w.Location.Y, z = w.Location.Z, node = traveller.TrackNodeIndex,
            offset_m = traveller.TrackNodeOffset, direction = traveller.Direction.ToString() };
    }
    static object Vehicle(TrainCar car) {
        var diesel = car as MSTSDieselLocomotive;
        return new { id = car.CarID, file = Path.GetFileName(car.WagFilePath),
            owner = car.Train.Number, mass_kg = car.MassKG, flipped = car.Flipped,
            speed_mps = car.SpeedMpS, fuel_l = diesel == null ? (float?)null : diesel.DieselLevelL,
            brake_type = car.BrakeSystem.GetType().Name,
            pipe_psi = car.BrakeSystem.BrakeLine1PressurePSI,
            cylinder_psi = car.BrakeSystem.GetCylPressurePSI(),
            auxiliary_psi = Convert.ToSingle(Field(car.BrakeSystem, "AuxResPressurePSI")),
            shoe_force_n = car.BrakeShoeForceN, handbrake = car.BrakeSystem.GetHandbrakeStatus(),
            front_hose = car.BrakeSystem.FrontBrakeHoseConnected,
            rear_hose = car.BrakeSystem.RearBrakeHoseConnected,
            front_cock = car.BrakeSystem.AngleCockAOpen,
            rear_cock = car.BrakeSystem.AngleCockBOpen };
    }
    static object Track(Simulator sim, Train player, Train donor) {
        return sim.Signals.TrackCircuitList.Select(section => new {
            section = section.Index,
            player_occupied = section.CircuitState.ThisTrainOccupying(player),
            donor_occupied = section.CircuitState.ThisTrainOccupying(donor),
            reserved_by = section.CircuitState.TrainReserved == null ? (int?)null :
                section.CircuitState.TrainReserved.Train.Number
        }).Where(row => row.player_occupied || row.donor_occupied ||
            row.reserved_by == player.Number || row.reserved_by == donor.Number).ToArray();
    }
    static object State(Simulator sim, Train player, Train donor) {
        return new { player_number = player.Number, donor_number = donor.Number,
            active_trains = sim.Trains.Select(t => new { number = t.Number,
                type = t.TrainType.ToString(), cars = t.Cars.Select(c => c.CarID).ToArray() }).ToArray(),
            player_speed_mps = player.SpeedMpS, length_m = player.Length,
            mass_kg = player.Cars.Sum(c => (double)c.MassKG),
            vehicles = player.Cars.Select(Vehicle).ToArray(),
            donor_vehicles = sim.Trains.Contains(donor) ? donor.Cars.Select(Vehicle).ToArray() : new object[0],
            front = Point(player.FrontTDBTraveller), rear = Point(player.RearTDBTraveller),
            donor_front = Point(donor.FrontTDBTraveller), track = Track(sim, player, donor) };
    }
    static Traveller PrescribedTraveller(float x, float y, float z, float angle) {
        var t = Empty<Traveller>();
        SetField(t, "locationSet", true);
        SetField(t, "location", new WorldLocation(0, 0, x, y, z));
        SetField(t, "directionVector", new Vector3(0, angle, 0));
        return t;
    }
    static object Overlap() {
        var rear = PrescribedTraveller(0, 0, 0, 0);
        return new[] { new { id = "rear_front_overlap", x = 0f, y = 0f, z = .25f },
            new { id = "rear_front_separated", x = 0f, y = 0f, z = -.25f },
            new { id = "beyond_one_metre", x = 0f, y = 0f, z = 1.01f },
            new { id = "other_track", x = 2f, y = 0f, z = .25f },
            new { id = "other_elevation", x = 0f, y = 1.01f, z = .25f }
        }.Select(row => new { id = row.id, relative_m = new[] { row.x, row.y, row.z },
            overlap_m = rear.OverlapDistanceM(PrescribedTraveller(row.x, row.y, row.z, 0), true) }).ToArray();
    }
    static object Momentum() {
        return new[] { new { id = "stationary", a = 0f, b = 0f, multiplier = 1f },
            new { id = "low_speed_same_direction", a = -.05f, b = 0f, multiplier = 1f },
            new { id = "opposite_orientation", a = .05f, b = -.02f, multiplier = -1f }
        }.Select(input => {
            var a = Empty<Train>(); var b = Empty<Train>();
            a.Cars = new List<TrainCar>(); b.Cars = new List<TrainCar>();
            foreach (var mass in new[] { 40000f, 20000f, 20000f }) {
                var car = Empty<MSTSWagon>(); car.MassKG = mass; car.Train = a; a.Cars.Add(car);
            }
            foreach (var mass in new[] { 40000f, 20000f, 20000f }) {
                var car = Empty<MSTSWagon>(); car.MassKG = mass; car.Train = b; b.Cars.Add(car);
            }
            a.SpeedMpS = input.a; b.SpeedMpS = input.b;
            a.SetCoupleSpeed(b, input.multiplier);
            return new { id = input.id, masses_a_kg = a.Cars.Select(c => c.MassKG).ToArray(),
                masses_b_kg = b.Cars.Select(c => c.MassKG).ToArray(),
                initial_a_mps = input.a, initial_b_mps = input.b, other_multiplier = input.multiplier,
                resulting_a_mps = a.SpeedMpS, resulting_b_mps = b.SpeedMpS,
                cars_a_mps = a.Cars.Select(c => c.SpeedMpS).ToArray(),
                cars_b_mps = b.Cars.Select(c => c.SpeedMpS).ToArray() };
        }).ToArray();
    }
    static object Forces(Simulator sim, TrainCar[] original) {
        var train = new Train(sim, 999);
        train.Cars.Add(original[0]); train.Cars.Add(original[1]);
        foreach (var car in train.Cars) car.Train = train;
        var rows = new List<object>();
        foreach (var input in new[] {
            new { id = "stationary_unloaded", front = 0f, rear = 0f, sign = 0f },
            new { id = "engaged_tension", front = 80000f, rear = 0f, sign = 1f },
            new { id = "engaged_compression", front = 0f, rear = 80000f, sign = -1f }
        }) {
            foreach (var car in train.Cars) {
                car.SpeedMpS = 0; car.TotalForceN = 0; car.CouplerForceU = 0;
                car.FrictionForceN = 0; car.BrakeForceN = 0; car.CurveForceN = 0;
                car.WindForceN = 0; car.TunnelForceN = 0;
            }
            train.FirstCar.TotalForceN = input.front;
            train.LastCar.TotalForceN = input.rear;
            train.FirstCar.CouplerSlackM = input.sign *
                (train.FirstCar.GetMaximumSimpleCouplerSlack1M() + .01f);
            train.MaximumCouplerForceN = 0;
            train.ComputeCouplerForces(.05f);
            rows.Add(new { id = input.id, method = "Train.ComputeCouplerForces(0.05f)",
                simple_control_physics = true, masses_kg = train.Cars.Select(c => c.MassKG).ToArray(),
                input_force_n = new[] { input.front, input.rear }, slack_m = train.FirstCar.CouplerSlackM,
                coupler_force_or_n = train.FirstCar.CouplerForceU,
                resulting_forces_n = train.Cars.Select(c => c.TotalForceN).ToArray(),
                resulting_acceleration_mps2 = train.Cars.Select(c => c.TotalForceN / c.MassKG).ToArray() });
        }
        return rows.ToArray();
    }
    static object Run(Simulator sim) {
        var player = sim.PlayerLocomotive.Train;
        if (player.Cars.Count != 8) throw new InvalidOperationException("Expected the eight original Pullman vehicles");
        player.SpeedMpS = 0;
        foreach (var car in player.Cars) car.SpeedMpS = 0;
        var original = player.Cars.ToArray();
        var originalIds = original.Select(c => c.CarID).ToArray();
        ((MSTSLocomotive)sim.PlayerLocomotive).SetThrottlePercentWithSound(0);
        sim.UncoupleBehind(player.Cars[3], true);
        var donor = sim.Trains.Single(t => t != player);
        if (player.Cars.Count != 4 || donor.Cars.Count != 4)
            throw new InvalidOperationException("Real UncoupleBehind did not produce two four-car trains");
        foreach (var car in donor.Cars) car.BrakeSystem.SetHandbrakePercent(100);
        player.LastCar.BrakeSystem.AngleCockBOpen = false;
        player.LastCar.BrakeSystem.AngleCockBOpenAmount = 0;
        donor.FirstCar.BrakeSystem.AngleCockAOpen = false;
        donor.FirstCar.BrakeSystem.AngleCockAOpenAmount = 0;
        // Give both reference trains distinct, retained pneumatic/fuel inputs.
        for (int i = 0; i < original.Length; i++) {
            var car = original[i];
            car.BrakeSystem.BrakeLine1PressurePSI = 70 + i;
            SetField(car.BrakeSystem, "CylPressurePSI", 12f + i);
            SetField(car.BrakeSystem, "AuxResPressurePSI", 60f + i);
            var diesel = car as MSTSDieselLocomotive;
            if (diesel != null) diesel.DieselLevelL = diesel.MaxDieselLevelL * (i == 0 ? .6f : .4f);
        }
        var separatedFront = new Traveller(player.FrontTDBTraveller);
        if (Math.Abs(separatedFront.Move(2f)) > .001f)
            throw new InvalidOperationException("Cannot prescribe a separated reference contact");
        player.FrontTDBTraveller = separatedFront;
        player.RepositionRearTraveller();
        player.SpeedMpS = -.05f;
        sim.CheckForCoupling(player, .05f);
        if (!sim.Trains.Contains(donor) || player.UncoupledFrom != null || donor.UncoupledFrom != null)
            throw new InvalidOperationException("Separated scan did not keep the donor and clear the uncoupled guard");
        var separated = State(sim, player, donor);
        var contactFront = new Traveller(player.FrontTDBTraveller);
        if (Math.Abs(contactFront.Move(-2.25f)) > .001f)
            throw new InvalidOperationException("Cannot prescribe rear-to-front contact");
        player.FrontTDBTraveller = contactFront;
        player.RepositionRearTraveller();
        var overlap = player.RearTDBTraveller.OverlapDistanceM(donor.FrontTDBTraveller, true);
        if (!(overlap < 0 && overlap >= -1)) throw new InvalidOperationException("Reference endpoints do not overlap");
        player.SpeedMpS = 0;
        sim.CheckForCoupling(player, .05f);
        if (!sim.Trains.Contains(donor)) throw new InvalidOperationException("OR automatically coupled at zero speed");
        var stationary = State(sim, player, donor);
        player.SpeedMpS = -.05f;
        var before = State(sim, player, donor);
        sim.CheckForCoupling(player, .05f);
        var after = State(sim, player, donor);
        if (sim.Trains.Contains(donor) || player.Cars.Count != original.Length ||
            !player.Cars.Select(c => c.CarID).SequenceEqual(originalIds) ||
            player.Cars.Where((c, i) => !Object.ReferenceEquals(c, original[i]) || c.Train != player).Any())
            throw new InvalidOperationException("Real coupling failed to transfer the original objects exactly once");
        if (sim.Signals.TrackCircuitList.Any(s => s.CircuitState.ThisTrainOccupying(donor) ||
            (s.CircuitState.TrainReserved != null && s.CircuitState.TrainReserved.Train == donor)))
            throw new InvalidOperationException("Removed donor retains occupation or a reservation");
        var forces = Forces(sim, original);
        return new { split_method = "Simulator.UncoupleBehind(TrainCar, true)",
            contact_method = "Simulator.CheckForCoupling(Train, 0.05f)",
            original_order = originalIds, signed_overlap_m = overlap,
            separated = separated, stationary_scan = stationary, before = before, after = after,
            every_original_object_transferred_once = true, removed_donor_has_no_occupation_or_reservations = true,
            equilibrium_forces = forces };
    }
    static int Main(string[] args) {
        try {
            if (args.Length != 2) throw new ArgumentException("CouplingReference CONFIG.json OUTPUT.json");
            Thread.CurrentThread.CurrentCulture = Culture; Thread.CurrentThread.CurrentUICulture = Culture;
            Thread.CurrentThread.Name = "Loader Process";
            Trace.Listeners.Add(new TextWriterTraceListener(Console.Error)); Trace.AutoFlush = true;
            var input = JObject.Parse(File.ReadAllText(args[0]));
            var settings = new UserSettings(new[] { "Language=en", "UseAdvancedAdhesion=false",
                "SimpleControlPhysics=true", "ActRandomizationLevel=0", "ActWeatherRandomizationLevel=0",
                "BreakCouplers=false", "Alerter=false", "HotStart=true", "ElectricHotStart=true" });
            var path = (string)input["native_path"];
            var sim = new Simulator(settings, path, false, true);
            sim.Confirmer = new Confirmer(sim, 1);
            sim.SetExplore(path, (string)input["native_consist"], "9:55", "1", "0");
            sim.Start(new ORTS.Common.CancellationTokenSource(delegate { }).Token);
            sim.SetCommandReceivers(); sim.PreUpdate = false;
            object coupling = null; Exception failure = null;
            var thread = new Thread(delegate() { try { coupling = Run(sim); } catch (Exception e) { failure = e; } });
            thread.Name = "Updater Process"; thread.Start(); thread.Join();
            if (failure != null) throw failure;
            File.WriteAllText(args[1], JsonConvert.SerializeObject(new {
                reference = "Open Rails 1.6.1", dt_s = .05, coupling = coupling,
                overlap = Overlap(), momentum = Momentum(),
                scope = "Real unmodified CheckForCoupling transfer and track-state update; prescribed contact and retained subsystem inputs. Moving-impact and full-route physics are not certified."
            }, Formatting.Indented).Replace("\r\n", "\n") + "\n");
            return 0;
        } catch (Exception error) { Console.Error.WriteLine(error); return 1; }
    }
}
