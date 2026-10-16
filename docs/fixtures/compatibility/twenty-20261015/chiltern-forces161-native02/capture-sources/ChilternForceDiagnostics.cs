// A deterministic client of the unmodified Open Rails 1.6.1 simulation DLL.
// Build beside the pinned installation's assemblies (see capture script).
using System;
// BEGIN CHILTERN_ADHESION_OBSERVER_USING
using System.Reflection;
// END CHILTERN_ADHESION_OBSERVER_USING
using System.IO;
using System.Linq;
using System.Threading;
using System.Diagnostics;
using System.Globalization;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using ORTS.Settings;
using ORTS.Common;
using Orts.Formats.Msts;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;
using Orts.Simulation.RollingStocks.SubSystems;

internal static class HeadlessCapture
{
    static readonly CultureInfo Culture = CultureInfo.InvariantCulture;
    static readonly int[] Platforms = { 1286, 1290, 1080 };
    static readonly int[] Hosts = { 96, 100, 104 };

    // BEGIN CHILTERN_ADHESION_OBSERVER_METHODS
    static readonly JArray AdhesionRows = new JArray();
    static object RequiredField(object instance, string name)
    {
        for (Type type = instance.GetType(); type != null; type = type.BaseType) {
            var field = type.GetField(name, BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly);
            if (field != null) return field.GetValue(instance);
        }
        throw new MissingFieldException(instance.GetType().FullName, name);
    }
    static object DiagnosticField(object instance, string name)
    {
        for (Type type = instance.GetType(); type != null; type = type.BaseType) {
            var field = type.GetField(name, BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.DeclaredOnly);
            if (field != null) return field.GetValue(instance);
        }
        return null; // An unavailable diagnostic is never substituted by a model.
    }
    static void ObserveAdhesion(Simulator sim, double elapsed)
    {
        var train = sim.PlayerLocomotive.Train;
        var vehicles = new JArray();
        for (int index = 0; index < train.Cars.Count; index++) {
            var loco = train.Cars[index] as MSTSLocomotive;
            if (loco == null) continue;
            var filter = loco.AdhesionFilter;
            if (filter == null) throw new InvalidOperationException("Original adhesion filter missing");
            var x = ((float[])RequiredField(filter, "x")).ToArray();
            var y = ((float[])RequiredField(filter, "y")).ToArray();
            if (x.Length == 0 || y.Length == 0) throw new InvalidDataException("Original filter buffers are empty");
            var values = new JObject {
                { "vehicle", index }, { "source", Path.GetFileName(loco.WagFilePath) },
                { "input_factor", x[0] },
                { "base_factor", Convert.ToSingle(RequiredField(loco, "BaseFrictionCoefficientFactor"), Culture) },
                { "output_factor", y[0] }, { "x", JArray.FromObject(x) }, { "y", JArray.FromObject(y) },
                { "a", JArray.FromObject(filter.A.ToArray()) }, { "b", JArray.FromObject(filter.B.ToArray()) },
                { "order", filter.Order }, { "filter_type", filter.FilterType.ToString() },
                { "cutoff_rad_s", filter.CutoffFrequencyRadpS }, { "sampling_period_s", filter.SamplingPeriod_s }
            };
            var diagnostic = new JObject();
            foreach (string name in new[] { "SaveAdhesionFilter", "AdhesionConditions", "AbsSpeedMpS", "WheelSpeedMpS",
                "AbsTractionSpeedMpS", "PrevAbsTractionSpeedMpS", "WheelSlip", "WheelSlipWarning", "TractiveForceN",
                "MotiveForceN", "BrakeShoeForceN", "BrakeRetardForceN", "BrakeForceN", "ZeroSpeedAdhesionBase" }) {
                var value = DiagnosticField(loco, name);
                diagnostic.Add(name, value == null ? JValue.CreateNull() : JToken.FromObject(value));
            }
            values.Add("diagnostics_not_inputs", diagnostic);
            vehicles.Add(values);
        }
        AdhesionRows.Add(new JObject { { "tick", (int)Math.Round(elapsed / 0.05) },
            { "time_s", elapsed }, { "vehicles", vehicles } });
    }
    // BEGIN CHILTERN_BODY_OBSERVER_METHODS
    static StreamWriter CarDiagnosticsWriter;
    static readonly string[] BodyRequiredFields = {
        "CurveForceN", "CurveForceNFiltered", "WindForceN", "TunnelForceN", "GravityForceN", "FrictionForceN",
        "BrakeForceN", "MotiveForceN", "TotalForceN", "_SpeedMpS", "AbsSpeedMpS", "MassKG",
        "CurrentCurveRadiusM", "SuperElevationM", "SuperElevationAngleRad", "TrackGaugeM", "RigidWheelBaseM",
        "BrakeShoeForceN", "BrakeRetardForceN", "BrakeSkid", "BrakeSkidWarning", "WheelSlip", "WheelSlipWarning",
        "HuDBrakeShoeFriction", "CurrentElevationPercent", "Flipped", "CarLengthM", "CarBogieCentreLengthM"
    };
    static readonly string[] BodyOptionalFields = {
        "BrakeShoeCoefficientFriction", "BrakeShoeCoefficientFrictionAdjFactor", "BrakeShoeRetardCoefficientFrictionAdjFactor",
        "BrakeWheelTreadForceN", "WagonBrakeAdhesiveForceN", "_PrevSpeedMpS"
    };
    static void StartCarDiagnostics(string output)
    {
        CarDiagnosticsWriter = new StreamWriter(Path.Combine(output, "car-diagnostics.csv"));
        CarDiagnosticsWriter.WriteLine("tick,time_s,vehicle,source,brake_system," + String.Join(",", BodyRequiredFields)
            + "," + String.Join(",", BodyOptionalFields) + ",WagonCoefficientFriction,CylPressurePSI");
    }
    static void ObserveCarDiagnostics(Simulator sim, double elapsed)
    {
        var train = sim.PlayerLocomotive.Train;
        for (int index = 0; index < train.Cars.Count; index++) {
            var car = train.Cars[index];
            var values = new System.Collections.Generic.List<object> {
                (int)Math.Round(elapsed / 0.05), elapsed, index, Path.GetFileName(car.WagFilePath), car.BrakeSystem.GetType().FullName
            };
            foreach (string name in BodyRequiredFields) values.Add(RequiredField(car, name));
            foreach (string name in BodyOptionalFields) values.Add(DiagnosticField(car, name));
            values.Add(RequiredField(train, "WagonCoefficientFriction"));
            values.Add(DiagnosticField(car.BrakeSystem, "CylPressurePSI"));
            CarDiagnosticsWriter.WriteLine(String.Join(",", values.Select(Number)));
        }
    }
    // END CHILTERN_BODY_OBSERVER_METHODS

    static void SaveAdhesion(string output, int vehicleCount)
    {
        if (AdhesionRows.Count == 0) throw new InvalidDataException("No adhesion samples");
        // BEGIN CHILTERN_BODY_OBSERVER_SAVE
        CarDiagnosticsWriter.Dispose();
        // END CHILTERN_BODY_OBSERVER_SAVE
        Json(Path.Combine(output, "adhesion-observation.json"), new {
            schema = 1, step_s = 0.05, vehicle_count = vehicleCount,
            method = "Read-only original IIRFilter.x/y and BaseFrictionCoefficientFactor; no field writes, RNG calls, filter reset or Filter invocation added",
            scope = "Input replay may use initial x/y, post-step input_factor and base_factor only; post-step output_factor/y and diagnostics_not_inputs validate the native filter and cannot drive the Rust trajectory",
            initial_tick = 0, initial_time_s = 0.0, initial = AdhesionRows[0]["vehicles"],
            rows_including_initial = AdhesionRows.Count, samples = AdhesionRows.Skip(1).ToArray() });
    }
    // END CHILTERN_ADHESION_OBSERVER_METHODS

    static void Json(string path, object value)
    {
        File.WriteAllText(path, JsonConvert.SerializeObject(value, Formatting.Indented));
    }

    static object Point(Traveller traveller)
    {
        var location = traveller.WorldLocation;
        return new { tile_x = location.TileX, tile_z = location.TileZ,
            x = location.Location.X, y = location.Location.Y, z = location.Location.Z,
            node = traveller.TrackNodeIndex, offset_m = traveller.TrackNodeOffset,
            direction = traveller.Direction.ToString() };
    }

    static Simulator Load(string path, string consist, bool activity)
    {
        var settings = new UserSettings(new[] { "DataLogPerformance=false",
            "DataLogTrainSpeed=false", "DataLogStationStops=false", "Language=en",
            "UseAdvancedAdhesion=true", "SimpleControlPhysics=false", "AdhesionFactor=100",
            "AdhesionFactorChange=10", "AdhesionProportionalToWeather=false", "GraduatedRelease=false",
            "BrakePipeChargingRate=21", "CorrectQuestionableBrakingParams=false",
            "CurveSpeedDependent=false", "TunnelResistanceDependent=false", "WindResistanceDependent=false",
            "NoForcedRedAtStationStops=false", "ActRandomizationLevel=0", "ActWeatherRandomizationLevel=0",
            "BreakCouplers=false", "Alerter=false", "HotStart=true", "ElectricHotStart=true" });
        var sim = new Simulator(settings, path, false, true);
        sim.Confirmer = new Confirmer(sim, 1.0);
        if (activity) sim.SetActivity(path);
        else sim.SetExplore(path, consist, "9:55", "1", "0");
        sim.Start(new ORTS.Common.CancellationTokenSource(delegate { }).Token);
        sim.SetCommandReceivers();
        sim.PreUpdate = false;
        return sim;
    }

    static Traveller Platform(Simulator sim, int index)
    {
        var item = sim.TDB.TrackDB.TrItemTable[Platforms[index]];
        return new Traveller(sim.TSectionDat, sim.TDB.TrackDB.TrackNodes,
            sim.TDB.TrackDB.TrackNodes[Hosts[index]], item.TileX, item.TileZ,
            item.X, item.Z, Traveller.TravellerDirection.Backward);
    }

    static void Prepare(string path, string consist, string output)
    {
        var sim = Load(path, consist, false);
        var train = sim.PlayerLocomotive.Train;
        var rear = Platform(sim, 0);
        if (Math.Abs(rear.Move(-train.Length)) > 0.01)
            throw new InvalidOperationException("Cannot position the complete consist behind Northolt");
        var junction = sim.TDB.TrackDB.TrackNodes[95].UiD;
        Json(output, new { cars = train.Cars.Count, length_m = train.Length,
            mass_kg = train.Cars.Sum(car => (double)car.MassKG), rear = Point(rear),
            initial_front = Point(train.FrontTDBTraveller),
            front = Point(Platform(sim, 0)),
            junction = new { tile_x = junction.TileX, tile_z = junction.TileZ,
                x = junction.X, y = junction.Y, z = junction.Z },
            stations = Enumerable.Range(0, 3).Select(index => new {
                item_id = Platforms[index], host = Hosts[index],
                name = ((PlatformItem)sim.TDB.TrackDB.TrItemTable[Platforms[index]]).Station,
                point = Point(Platform(sim, index)) }),
            vehicles = train.Cars.Select(car => new { length_m = car.CarLengthM,
                mass_kg = car.MassKG, source = Path.GetFileName(car.WagFilePath) }),
            speed_posts = sim.TDB.TrackDB.TrItemTable.OfType<SpeedPostItem>()
                .Where(item => item.IsLimit && item.SigObj >= 0)
                .Where(item => new[] { 94, 96, 98, 100, 102, 104 }.Contains(sim.Signals.SignalObjects[item.SigObj].trackNode))
                .Select(item => new { id = item.TrItemId,
                    node = sim.Signals.SignalObjects[item.SigObj].trackNode,
                    direction = sim.Signals.SignalObjects[item.SigObj].direction,
                    chainage_forward_m = item.SData1,
                    speed_mps = item.IsMPH ? item.SpeedInd * 0.44704 : item.SpeedInd / 3.6,
                    resume = item.IsResume, passenger = item.IsPassenger, freight = item.IsFreight }) });
    }

    static double DistanceToPlatform(Simulator sim, int index)
    {
        var item = sim.TDB.TrackDB.TrItemTable[Platforms[index]];
        var head = new Traveller(sim.PlayerLocomotive.Train.FrontTDBTraveller);
        return head.DistanceTo(sim.TDB.TrackDB.TrackNodes[Hosts[index]],
            item.TileX, item.TileZ, item.X, item.Y, item.Z);
    }

    static void Controls(MSTSLocomotive loco, double throttle, double brake)
    {
        loco.SetDirection(Direction.Forward);
        loco.SetThrottlePercentWithSound((float)(100 * throttle));
        // RF_WP_DMBSA's native EP service range is 0.05..0.45. The trace stores
        // both semantic service demand and the actual native handle position.
        loco.SetTrainBrakePercent((float)(brake <= 0 ? 0 : 5 + 40 * brake));
    }

    static string Number(object value)
    {
        if (value is float) return ((float)value).ToString("R", Culture);
        if (value is double) return ((double)value).ToString("R", Culture);
        return Convert.ToString(value, Culture);
    }

    static void Sample(StreamWriter writer, Simulator sim, double elapsed,
        double throttle, double brake, bool doors)
    {
        var loco = (MSTSLocomotive)sim.PlayerLocomotive;
        var train = loco.Train;
        var head = train.FrontTDBTraveller;
        string edge = "e" + head.TrackNodeIndex +
            (head.Direction == Traveller.TravellerDirection.Backward ? "_r" : "");
        var signal = train.GetNextSignalAspect(0);
        var values = new object[] { elapsed, edge, head.TrackNodeOffset,
            train.SpeedMpS, train.DistanceTravelledM, throttle, brake,
            loco.TrainBrakeController.CurrentValue, loco.BrakeSystem.BrakeLine1PressurePSI,
            loco.BrakeSystem.GetCylPressurePSI(), doors ? 1 : 0, signal.ToString(),
            train.DistanceToSignal ?? -1,
            train.Cars.Sum(car => (double)car.MassKG), train.AllowedMaxSpeedMpS,
            train.DoorState(DoorSide.Both).ToString(),
            train.Cars.Sum(car => (double)car.MotiveForceN),
            train.Cars.Sum(car => (double)car.BrakeForceN),
            train.Cars.Sum(car => (double)car.FrictionForceN),
            train.Cars.Sum(car => (double)car.GravityForceN) };
        writer.WriteLine(String.Join(",", values.Select(Number)));
        // BEGIN CHILTERN_ADHESION_OBSERVER_SAMPLE
        ObserveAdhesion(sim, elapsed);
        // BEGIN CHILTERN_BODY_OBSERVER_SAMPLE
        ObserveCarDiagnostics(sim, elapsed);
        // END CHILTERN_BODY_OBSERVER_SAMPLE
        // END CHILTERN_ADHESION_OBSERVER_SAMPLE
    }

    static void Capture(string activity, string output, string mode, string config, string replay)
    {
        var sim = Load(activity, null, true);
        var loco = (MSTSLocomotive)sim.PlayerLocomotive;
        var train = loco.Train;
        if (train.Cars.Count != 8) throw new InvalidOperationException("Expected eight Pullman cars");
        double initialError = DistanceToPlatform(sim, 0);
        if (initialError < 0 || initialError > 0.10)
            throw new InvalidOperationException("Initial head differs from Northolt: " + initialError);
        Directory.CreateDirectory(output);
        // BEGIN CHILTERN_BODY_OBSERVER_START
        StartCarDiagnostics(output);
        // END CHILTERN_BODY_OBSERVER_START
        Json(Path.Combine(output, "initial.json"), new {
            front = Point(train.FrontTDBTraveller), error_m = initialError,
            cars = train.Cars.Count, length_m = train.Length,
            settings = new { sim.Settings.UseAdvancedAdhesion, sim.Settings.SimpleControlPhysics,
                sim.Settings.AdhesionFactor, sim.Settings.AdhesionFactorChange,
                sim.Settings.AdhesionProportionalToWeather, sim.Settings.GraduatedRelease,
                sim.Settings.BrakePipeChargingRate, sim.Settings.CorrectQuestionableBrakingParams,
                sim.Settings.CurveSpeedDependent, sim.Settings.TunnelResistanceDependent,
                sim.Settings.WindResistanceDependent, sim.Settings.NoForcedRedAtStationStops,
                sim.Settings.ActRandomizationLevel, sim.Settings.ActWeatherRandomizationLevel },
            vehicles = train.Cars.Select(car => new { length_m = car.CarLengthM,
                mass_kg = car.MassKG, max_brake_force_n = car.MaxBrakeForceN,
                brake_type = car.BrakeSystem.GetType().Name,
                source = Path.GetFileName(car.WagFilePath) }) });

        Exception failure = null;
        var thread = new Thread(delegate() {
            try { Run(sim, loco, output, mode, JObject.Parse(File.ReadAllText(config)), replay); }
            catch (Exception error) { failure = error; }
        });
        thread.Name = "Updater Process";
        thread.Start(); thread.Join();
        if (failure != null) throw new InvalidOperationException("Reference capture failed", failure);
    }

    static void Run(Simulator sim, MSTSLocomotive loco, string output, string mode, JObject config, string replay)
    {
        const double dt = 0.05;
        var timetable = (JArray)config["stops"];
        if (timetable.Count != Platforms.Length || (double)config["step_s"] != dt)
            throw new InvalidDataException("Capture requires the three selected platforms at 20 Hz");
        var stops = new JArray();
        string[] rows = mode == "replay" ? File.ReadAllLines(replay) : null;
        string[] header = rows == null ? null : rows[0].Split(',');
        int timeColumn = header == null ? 0 : Array.IndexOf(header, "time_s");
        int throttleColumn = header == null ? 0 : Array.IndexOf(header, "throttle");
        int brakeColumn = header == null ? 0 : Array.IndexOf(header, "brake");
        if (rows != null && Math.Min(timeColumn, Math.Min(throttleColumn, brakeColumn)) < 0)
            throw new InvalidDataException("Replay needs time_s, throttle, brake columns");
        int row = 1, stopIndex = 0;
        bool boarding = false, doors = false;
        double boardingElapsed = 0, throttle = 0, brake = 0;
        double initialClock = sim.ClockTime;
        string failure = null;
        using (var writer = new StreamWriter(Path.Combine(output, "trace.csv"))) {
            writer.WriteLine("time_s,edge_id,pos_on_edge_m,velocity_mps,odometer_m,throttle,brake,native_brake_handle,brake_pipe_psi,cylinder_psi,doors_command_open,next_signal,next_signal_distance_m,mass_kg,allowed_speed_mps,door_state,motive_force_n,brake_force_n,friction_force_n,gravity_force_n");
            Sample(writer, sim, 0, 0, 0, false);
            for (int tick = 0; tick < 36000 && stopIndex < 3; tick++) {
                double t = tick * dt;
                double speed = Math.Abs(loco.Train.SpeedMpS);
                double distance = DistanceToPlatform(sim, stopIndex);
                if (mode == "replay") {
                    while (row + 1 < rows.Length &&
                        Double.Parse(rows[row + 1].Split(',')[timeColumn], Culture) <= t + 1e-6) row++;
                    var controls = rows[row].Split(',');
                    throttle = Double.Parse(controls[throttleColumn], Culture);
                    brake = Double.Parse(controls[brakeColumn], Culture);
                    if (t > Double.Parse(rows[rows.Length - 1].Split(',')[timeColumn], Culture) + 1e-6) break;
                } else {
                    if (!boarding && distance >= 0 && distance <= 10 && speed <= 0.1) {
                        boarding = true; boardingElapsed = 0; doors = true;
                        loco.Train.SetDoors(DoorSide.Both, true);
                        stops.Add(JObject.FromObject(new { station = ((PlatformItem)sim.TDB.TrackDB.TrItemTable[Platforms[stopIndex]]).Station,
                            arrival_s = t, position_error_m = distance, arrival_speed_mps = speed }));
                        Console.WriteLine("Arrived at platform " + Platforms[stopIndex] + " at " + t + " s");
                    }
                    if (boarding) {
                        throttle = 0; brake = 1;
                        if (loco.Train.DoorState(DoorSide.Both) == DoorState.Open)
                            boardingElapsed += dt;
                        if (t + 1e-9 >= (double)timetable[stopIndex]["depart_s"] &&
                            boardingElapsed + 1e-9 >= (double)timetable[stopIndex]["dwell_s"]) {
                            doors = false; loco.Train.SetDoors(DoorSide.Both, false);
                            if (loco.Train.DoorState(DoorSide.Both) == DoorState.Closed) {
                                ((JObject)stops[stopIndex])["depart_s"] = t;
                                ((JObject)stops[stopIndex])["boarding_s"] = boardingElapsed;
                                stopIndex++; boarding = false;
                            }
                        }
                    } else {
                        if (distance < 0) { failure = "Missed platform " + Platforms[stopIndex]; break; }
                        double cap = Math.Min(65 / 3.6, loco.Train.AllowedMaxSpeedMpS) * 0.9;
                        cap = Math.Min(cap, Math.Sqrt(2 * 0.22 * Math.Max(0, distance - 4)));
                        if (loco.Train.GetNextSignalAspect(0) == MstsSignalAspect.STOP && loco.Train.DistanceToSignal.HasValue)
                            cap = Math.Min(cap, Math.Sqrt(2 * 0.22 * Math.Max(0, loco.Train.DistanceToSignal.Value - 4)));
                        throttle = speed < cap - 0.3 ? 0.75 : 0;
                        brake = speed > cap + 0.15 ? (cap < 0.5 ? 1 : 0.45) : 0;
                    }
                }
                Controls(loco, throttle, brake);
                sim.Update((float)dt);
                Sample(writer, sim, (tick + 1) * dt, throttle, brake, doors);
                if (loco.Train.ControlMode == Train.TRAIN_CONTROL.OUT_OF_CONTROL) {
                    failure = "Original train out of control: " + loco.Train.OutOfControlReason;
                    break;
                }
            }
        }
        // BEGIN CHILTERN_ADHESION_OBSERVER_SAVE
        SaveAdhesion(output, loco.Train.Cars.Count);
        // END CHILTERN_ADHESION_OBSERVER_SAVE
        if (mode == "service" && stopIndex < 3 && failure == null)
            failure = "Service did not complete within 1800 seconds";
        var nativeTasks = sim.ActivityRun.Tasks.OfType<ActivityTaskPassengerStopAt>().ToArray();
        bool nativeCompleted = nativeTasks.Length == 3 && nativeTasks.All(task => task.IsCompleted == true);
        if (mode == "service" && !nativeCompleted && failure == null)
            failure = "Original Open Rails station tasks did not all complete successfully";
        Json(Path.Combine(output, "outcome.json"), new { success = stopIndex == 3 && nativeCompleted,
            mode = mode, time_s = sim.ClockTime - initialClock, stops = stops,
            failure = failure, step_s = dt, seed = 0,
            native_tasks = nativeTasks.Select(task => new { station = task.PlatformEnd1.Station,
                completed = task.IsCompleted,
                arrival_s = task.ActArrive.HasValue ? (double?)(task.ActArrive.Value.TimeOfDay.TotalSeconds - initialClock) : null,
                departure_s = task.ActDepart.HasValue ? (double?)(task.ActDepart.Value.TimeOfDay.TotalSeconds - initialClock) : null,
                boarding_s = task.BoardingS }),
            distance_m = loco.Train.DistanceTravelledM });
    }

    static int Main(string[] args)
    {
        try {
            Thread.CurrentThread.CurrentCulture = Culture;
            Thread.CurrentThread.CurrentUICulture = Culture;
            Thread.CurrentThread.Name = "Loader Process";
            Trace.Listeners.Add(new TextWriterTraceListener(Console.Error));
            Trace.AutoFlush = true;
            if (args.Length == 4 && args[0] == "prepare") Prepare(args[1], args[2], args[3]);
            else if ((args.Length == 4 && args[0] == "service") || (args.Length == 5 && args[0] == "replay"))
                Capture(args[1], args[2], args[0], args[3], args.Length > 4 ? args[4] : null);
            else throw new ArgumentException("prepare PAT CON OUT.json | service ACT OUT config.json | replay ACT OUT config.json controls.csv");
            return 0;
        } catch (Exception error) { Console.Error.WriteLine(error); return 1; }
    }
}
