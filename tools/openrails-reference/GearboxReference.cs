// A client of the unmodified Open Rails 1.6.1 assemblies. All gearbox, diesel
// engine, parser and train-physics results below come from their native methods.
// The prescribed subsystem inputs are explicitly separate from Simulator.Update.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Serialization;
using System.Text;
using System.Threading;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using ORTS.Settings;
using ORTS.Common;
using ORTS.Scripting.Api;
using Orts.Parsers.Msts;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;
using Orts.Simulation.RollingStocks.SubSystems.Controllers;
using Orts.Simulation.RollingStocks.SubSystems.PowerSupplies;
using Orts.Simulation.RollingStocks.SubSystems.PowerTransmissions;

class GearboxReference
{
    static readonly CultureInfo Culture = CultureInfo.InvariantCulture;
    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static void Set(object obj, string name, object value) {
        for (Type type = obj.GetType(); type != null; type = type.BaseType) {
            var property = type.GetProperty(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if (property != null) { property.SetValue(obj, value, null); return; }
            var field = type.GetField(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if (field != null) { field.SetValue(obj, value); return; }
        }
        throw new MissingMemberException(obj.GetType().Name, name);
    }
    static void Json(string path, object value) {
        File.WriteAllText(path, JsonConvert.SerializeObject(value, Formatting.Indented).Replace("\r\n", "\n") + "\n");
    }
    static string Number(object value) {
        if (value is float) return ((float)value).ToString("R", Culture);
        if (value is double) return ((double)value).ToString("R", Culture);
        return Convert.ToString(value, Culture);
    }
    static object Table(Interpolator table) {
        return Enumerable.Range(0, table.GetSize()).Select(i => new[] { table.X[i], table.Y[i] }).ToArray();
    }
    static MSTSDieselLocomotive PrescribedLocomotive(JObject profile) {
        var locomotive = Empty<MSTSDieselLocomotive>();
        var simulator = Empty<Simulator>();
        Set(simulator, "Settings", new UserSettings(new[] { "Language=en", "VerboseConfigurationMessages=false", "NoDieselEngineStart=false" }));
        Set(locomotive, "Simulator", simulator);
        locomotive.Train = Empty<Train>();
        locomotive.Train.Cars = new List<TrainCar> { locomotive };
        locomotive.Train.LeadLocomotiveIndex = 0;
        locomotive.EventHandlers = new List<Orts.Common.EventHandler>();
        locomotive.RemoteControlGroup = -1;
        locomotive.Direction = Direction.Forward;
        var supply = new ScriptedDieselPowerSupply(locomotive);
        locomotive.PowerSupply = supply;
        supply.MainPowerSupplyState = PowerSupplyState.PowerOn;
        supply.MaxThrottlePercent = 100;
        locomotive.DieselEngines = new DieselEngines(locomotive);
        foreach (JObject field in (JArray)profile["fields"]) {
            using (var input = new MemoryStream(Encoding.UTF8.GetBytes("( " + (string)field["raw"] + " )")))
            using (var reader = new STFReader(input, "gearbox-authored-parameter", Encoding.UTF8, false))
                locomotive.Parse((string)field["token"], reader);
        }
        if (profile["transmission_override"] != null)
            locomotive.DieselTransmissionType = (MSTSDieselLocomotive.DieselTransmissionTypes)Enum.Parse(
                typeof(MSTSDieselLocomotive.DieselTransmissionTypes), (string)profile["transmission_override"]);
        var engine = new DieselEngine(locomotive);
        locomotive.DieselEngines.Add(engine);
        engine.InitFromMSTS();
        engine.InitDieselRailPowers(locomotive);
        engine.Initialize();
        if (engine.GearBox == null) throw new InvalidDataException("Native parser did not initialize a gearbox");
        locomotive.GearBoxController = new MSTSNotchController(engine.GearBox.NumOfGears + 1);
        return locomotive;
    }
    static object Parameters(MSTSDieselLocomotive locomotive) {
        var engine = locomotive.DieselEngines[0];
        var parameters = locomotive.DieselEngines.MSTSGearBoxParams;
        return new {
            transmission = locomotive.DieselTransmissionType.ToString(),
            operation = parameters.GearBoxOperation.ToString(),
            gearbox_type = parameters.GearBoxType.ToString(), clutch_type = parameters.ClutchType.ToString(),
            number_of_gears = parameters.GearBoxNumberOfGears, direct_drive_gear = parameters.GearBoxDirectDriveGear,
            reverse_indication = parameters.ReverseGearBoxIndication,
            freewheel_fitted = parameters.FreeWheelFitted, engine_braking = parameters.GearBoxEngineBraking.ToString(),
            max_speed_mps = parameters.GearBoxMaxSpeedForGearsMpS,
            max_force_n = parameters.GearBoxMaxTractiveForceForGearsN,
            force_at_max_speed_n = parameters.GearBoxTractiveForceAtSpeedN,
            overspeed_pct = parameters.GearBoxOverspeedPercentageForFailure,
            backload_force_n = parameters.GearBoxBackLoadForceN, coasting_force_n = parameters.GearBoxCoastingForceN,
            up_gear_proportion = parameters.GearBoxUpGearProportion, down_gear_proportion = parameters.GearBoxDownGearProportion,
            idle_rpm = engine.IdleRPM, max_rpm = engine.MaxRPM, starting_rpm = engine.StartingRPM,
            governor_rpm = engine.GovernorRPM, change_up_rpm_s = engine.ChangeUpRPMpS,
            change_down_rpm_s = engine.ChangeDownRPMpS, maximum_power_w = engine.MaximumDieselPowerW,
            wheel_radius_m = locomotive.DriverWheelRadiusM, engine_count = locomotive.DieselEngines.Count,
            consumption_lph = Table(engine.DieselConsumptionTab), torque_nm = Table(engine.DieselTorqueTab),
            diesel_power_w = Table(engine.DieselPowerTab), throttle_rpm = Table(engine.ThrottleRPMTab),
            reverse_throttle_rpm = Table(engine.ReverseThrottleRPMTab)
        };
    }
    static object Gears(GearBox gearbox) {
        return gearbox.Gears.Select((gear, index) => new {
            index = index, ratio = gear.Ratio, max_speed_mps = gear.MaxSpeedMpS,
            max_force_n = gear.MaxTractiveForceN, force_at_max_speed_n = gear.TractiveForceatMaxSpeedN,
            change_up_rpm = gear.ChangeUpSpeedRpM, change_down_rpm = gear.ChangeDownSpeedRpM,
            overspeed_pct = gear.OverspeedPercentage, backload_force_n = gear.BackLoadForceN,
            coasting_force_n = gear.CoastingForceN, up_gear_proportion = gear.UpGearProportion,
            down_gear_proportion = gear.DownGearProportion, direct_drive = gear.IsDirectDriveGear
        }).ToArray();
    }
    // The order is part of the protocol: getters such as IsClutchOn have state.
    static JObject GearboxSample(GearBox gearbox, int tick) {
        float shaft = gearbox.ShaftRPM;
        float clutch = gearbox.ClutchPercent;
        bool clutchOn = gearbox.CurrentGear != null && gearbox.IsClutchOn;
        float force = gearbox.TractiveForceN;
        bool warning = gearbox.IsOverspeedWarning;
        bool error = gearbox.IsOverspeedError;
        return JObject.FromObject(new {
            tick = tick, current_gear_index = gearbox.CurrentGearIndex, next_gear_index = gearbox.NextGearIndex,
            shaft_rpm = shaft, clutch_pct = clutch, clutch_on = clutchOn, tractive_force_n = force,
            overspeed_warning = warning, overspeed_error = error, indication = gearbox.GearIndication,
            geared_up = gearbox.GearedUp, geared_down = gearbox.GearedDown
        });
    }
    static void Subsystems(string inputPath, string output) {
        var input = JObject.Parse(File.ReadAllText(inputPath));
        var profiles = new JArray();
        foreach (JObject profile in (JArray)input["profiles"]) {
            var locomotive = PrescribedLocomotive(profile);
            profiles.Add(JObject.FromObject(new {
                id = (string)profile["id"], source_vehicle = profile["source_vehicle"],
                parameters = Parameters(locomotive), gears = Gears(locomotive.DieselEngines[0].GearBox)
            }));
        }
        var scenarios = new JArray();
        foreach (JObject scenario in (JArray)input["scenarios"]) {
            JObject profile = ((JArray)input["profiles"]).OfType<JObject>().Single(p => (string)p["id"] == (string)scenario["profile_id"]);
            var locomotive = PrescribedLocomotive(profile);
            var engine = locomotive.DieselEngines[0];
            var gearbox = engine.GearBox;
            gearbox.GearBoxOperation = (GearBoxOperation)Enum.Parse(typeof(GearBoxOperation), (string)scenario["operation"]);
            float step = (float)scenario["step_s"];
            var points = new JArray();
            foreach (JObject prescribed in (JArray)scenario["inputs"]) {
                locomotive.LocalThrottlePercent = (float)prescribed["throttle_pct"];
                locomotive.Direction = (Direction)Enum.Parse(typeof(Direction), (string)prescribed["direction"]);
                locomotive.SpeedMpS = (float)prescribed["speed_mps"];
                locomotive.AbsSpeedMpS = Math.Abs(locomotive.SpeedMpS);
                locomotive.AbsWheelSpeedMpS = Math.Abs(locomotive.SpeedMpS);
                if (prescribed["rpm"] != null) engine.RealRPM = engine.DemandedRPM = (float)prescribed["rpm"];
                engine.DemandedThrottlePercent = locomotive.LocalThrottlePercent;
                if (prescribed["output_power_w"] != null) engine.CurrentDieselOutputPowerW = (float)prescribed["output_power_w"];
                if (prescribed["clutch_pct"] != null) gearbox.ClutchPercent = (float)prescribed["clutch_pct"];
                if (prescribed["next_gear_index"] != null) {
                    int next = (int)prescribed["next_gear_index"];
                    gearbox.NextGear = next < 0 ? null : gearbox.Gears[next];
                    locomotive.LocalGearboxGearIndex = next + 1;
                    locomotive.GearBoxController.CurrentNotch = next + 1;
                    locomotive.GearBoxController.SetValue(next + 1);
                }
                if (prescribed["manual_up"] != null) gearbox.ManualGearUp = (bool)prescribed["manual_up"];
                if (prescribed["manual_down"] != null) gearbox.ManualGearDown = (bool)prescribed["manual_down"];
                if ((string)scenario["method"] == "DieselEngine.Update") {
                    locomotive.TractionForceN = (float)prescribed["traction_force_n"];
                    engine.Update(step);
                } else gearbox.Update(step);
                var sample = GearboxSample(gearbox, (int)prescribed["tick"]);
                sample["rpm"] = engine.RealRPM;
                sample["demanded_rpm"] = engine.DemandedRPM;
                sample["flow_lps"] = engine.DieselFlowLps;
                sample["state"] = engine.State.ToString();
                points.Add(sample);
            }
            scenarios.Add(JObject.FromObject(new {
                id = (string)scenario["id"], profile_id = (string)scenario["profile_id"],
                kind = "prescribed_subsystem", operation = (string)scenario["operation"],
                method = (string)scenario["method"], step_s = step,
                input_sha256_is_recorded_in_manifest = true, checkpoints = points
            }));
        }
        Json(output, new {
            schema_version = 1, reference = "Open Rails 1.6.1",
            methods = new[] { "MSTSDieselLocomotive.Parse", "MSTSGearBoxParams.Parse", "DieselEngine.InitFromMSTS",
                "DieselEngine.InitDieselRailPowers", "GearBox.Initialize", "GearBox.Update", "DieselEngine.Update",
                "GearBox.ShaftRPM", "GearBox.ClutchPercent", "GearBox.IsClutchOn", "GearBox.TractiveForceN",
                "GearBox.IsOverspeedWarning", "GearBox.IsOverspeedError" },
            getter_order = new[] { "ShaftRPM", "ClutchPercent", "IsClutchOn_if_in_gear", "TractiveForceN", "IsOverspeedWarning", "IsOverspeedError", "GearIndication", "GearedUp", "GearedDown" },
            profiles = profiles, scenarios = scenarios,
            scope = "Native gearbox parsing, initialization, state changes and getters with prescribed speed/RPM/power/clutch inputs; native geared DieselEngine.Update with prescribed speed/traction. These are subsystem captures, not a complete route simulation."
        });
    }
    static object Point(Traveller traveller) {
        var location = traveller.WorldLocation;
        return new { node = traveller.TrackNodeIndex, offset_m = traveller.TrackNodeOffset,
            direction = traveller.Direction.ToString(), tile_x = location.TileX, tile_z = location.TileZ,
            x = location.Location.X, y = location.Location.Y, z = location.Location.Z };
    }
    static void DriveSample(StreamWriter writer, Simulator simulator, int tick, float throttle, float brake, int commandGear) {
        var locomotive = (MSTSDieselLocomotive)simulator.PlayerLocomotive;
        var engine = locomotive.DieselEngines[0];
        var gearbox = engine.GearBox;
        var train = locomotive.Train;
        // Observe fields/getters in a fixed order after the entire native update.
        float shaft = gearbox.ShaftRPM;
        float clutch = gearbox.ClutchPercent;
        writer.WriteLine(String.Join(",", new object[] {
            tick, tick * .05, throttle, brake, commandGear,
            train.SpeedMpS, train.DistanceTravelledM, engine.RealRPM, engine.DemandedRPM,
            engine.DemandedThrottlePercent, engine.CurrentDieselOutputPowerW,
            gearbox.CurrentGearIndex, gearbox.NextGearIndex, shaft, clutch, gearbox.clutchOn ? 1 : 0,
            locomotive.DieselFlowLps, locomotive.DieselLevelL, locomotive.MotiveForceN,
            locomotive.BrakeForceN, locomotive.FrictionForceN, locomotive.GravityForceN,
            locomotive.BrakeSystem.BrakeLine1PressurePSI, locomotive.BrakeSystem.GetCylPressurePSI(),
            train.FrontTDBTraveller.TrackNodeIndex, train.FrontTDBTraveller.TrackNodeOffset
        }.Select(Number)));
    }
    static void DriveRun(Simulator simulator, string output, JObject config) {
        var locomotive = (MSTSDieselLocomotive)simulator.PlayerLocomotive;
        var train = locomotive.Train;
        if (train.Cars.Count != 1 || locomotive.DieselEngines[0].GearBox == null)
            throw new InvalidOperationException("Expected the original single geared diesel vehicle");
        locomotive.SetDirection(Direction.Forward);
        locomotive.SetThrottlePercentWithSound(0);
        locomotive.SetTrainBrakePercent(100);
        for (int i = 0; i < (int)config["settle_ticks"]; i++) simulator.Update(.05f);
        Json(Path.Combine(output, "initial.json"), new {
            reference = "Open Rails 1.6.1", kind = "native_route_simulation", method = "Simulator.Update",
            source_vehicle = locomotive.WagFilePath, parameters = Parameters(locomotive),
            gears = Gears(locomotive.DieselEngines[0].GearBox),
            mass_kg = locomotive.MassKG, max_force_n = locomotive.MaxForceN,
            max_power_w = locomotive.MaxPowerW, length_m = locomotive.CarLengthM,
            brake_type = locomotive.BrakeSystem.GetType().Name,
            initial_speed_mps = train.SpeedMpS, initial_odometer_m = train.DistanceTravelledM,
            initial_fuel_l = locomotive.DieselLevelL, front = Point(train.FrontTDBTraveller),
            settings = new { simulator.Settings.UseAdvancedAdhesion, simulator.Settings.SimpleControlPhysics,
                simulator.Settings.AdhesionFactor, simulator.Settings.AdhesionFactorChange,
                simulator.Settings.AdhesionProportionalToWeather, simulator.Settings.GraduatedRelease,
                simulator.Settings.BrakePipeChargingRate, simulator.Settings.CorrectQuestionableBrakingParams,
                simulator.Settings.CurveSpeedDependent, simulator.Settings.TunnelResistanceDependent,
                simulator.Settings.WindResistanceDependent, simulator.Settings.BreakCouplers }
        });
        var commands = (JArray)config["commands"];
        int command = 0;
        double fuelIntegral = 0;
        int ticks = (int)config["ticks"];
        using (var writer = new StreamWriter(Path.Combine(output, "trace.csv"))) {
            writer.WriteLine("tick,time_s,throttle,brake,command_gear,velocity_mps,odometer_m,rpm,demanded_rpm,demanded_throttle_pct,output_power_w,current_gear_index,next_gear_index,shaft_rpm,clutch_pct,clutch_on,flow_lps,fuel_l,motive_force_n,brake_force_n,friction_force_n,gravity_force_n,pipe_psi,cylinder_psi,node,offset_m");
            DriveSample(writer, simulator, 0, 0, 1, 0);
            for (int tick = 0; tick < ticks; tick++) {
                while (command + 1 < commands.Count && (int)commands[command + 1]["tick"] <= tick) command++;
                var row = commands[command];
                float throttle = (float)row["throttle"], brake = (float)row["brake"];
                int gear = (int)row["gear"];
                locomotive.SetThrottlePercentWithSound(throttle * 100);
                locomotive.SetTrainBrakePercent(brake * 100);
                locomotive.GearBoxController.SetValue(gear);
                locomotive.GearboxGearIndex = gear;
                simulator.Update(.05f);
                fuelIntegral += locomotive.DieselFlowLps * .05;
                DriveSample(writer, simulator, tick + 1, throttle, brake, gear);
            }
        }
        Json(Path.Combine(output, "outcome.json"), new {
            reference = "Open Rails 1.6.1", kind = "native_route_simulation", method = "Simulator.Update",
            time_s = ticks * .05, final_speed_mps = train.SpeedMpS, odometer_m = train.DistanceTravelledM,
            fuel_l = locomotive.DieselLevelL, integrated_native_flow_l = fuelIntegral,
            control_mode = train.ControlMode.ToString(), out_of_control_reason = train.OutOfControlReason.ToString(),
            front = Point(train.FrontTDBTraveller),
            scope = "Single original Class 101 diesel: native acceleration, four manually commanded gears, coast, full vacuum brake and fuel consumption. Deterministic open-loop controls; no substituted speed, engine or force equations."
        });
    }
    static void Drive(string path, string consist, string output, string configPath) {
        Thread.CurrentThread.Name = "Loader Process";
        var settings = new UserSettings(new[] { "Language=en", "UseAdvancedAdhesion=false",
            "SimpleControlPhysics=true", "AdhesionFactor=100", "AdhesionFactorChange=0",
            "AdhesionProportionalToWeather=false", "GraduatedRelease=false", "BrakePipeChargingRate=21",
            "CorrectQuestionableBrakingParams=false", "CurveSpeedDependent=false",
            "TunnelResistanceDependent=false", "WindResistanceDependent=false", "NoForcedRedAtStationStops=true",
            "ActRandomizationLevel=0", "ActWeatherRandomizationLevel=0", "BreakCouplers=false",
            "Alerter=false", "HotStart=true", "ElectricHotStart=true", "DataLogPerformance=false",
            "DataLogTrainSpeed=false", "DataLogStationStops=false" });
        var simulator = new Simulator(settings, path, false, true);
        simulator.Confirmer = new Confirmer(simulator, 1);
        simulator.SetExplore(path, consist, "9:29", "1", "0");
        simulator.Start(new ORTS.Common.CancellationTokenSource(delegate { }).Token);
        simulator.SetCommandReceivers();
        simulator.PreUpdate = false;
        Directory.CreateDirectory(output);
        Exception failure = null;
        var thread = new Thread(delegate() {
            try { DriveRun(simulator, output, JObject.Parse(File.ReadAllText(configPath))); }
            catch (Exception error) { failure = error; }
        });
        thread.Name = "Updater Process";
        thread.Start(); thread.Join();
        if (failure != null) throw new InvalidOperationException("Native geared drive failed", failure);
    }
    static int Main(string[] args) {
        try {
            Thread.CurrentThread.CurrentCulture = Culture;
            Thread.CurrentThread.CurrentUICulture = Culture;
            Trace.Listeners.Add(new TextWriterTraceListener(Console.Error)); Trace.AutoFlush = true;
            if (args.Length == 3 && args[0] == "subsystems") Subsystems(args[1], args[2]);
            else if (args.Length == 5 && args[0] == "drive") Drive(args[1], args[2], args[3], args[4]);
            else throw new ArgumentException("subsystems INPUT.json OUTPUT.json | drive PAT CON OUT CONFIG.json");
            return 0;
        } catch (Exception error) { Console.Error.WriteLine(error); return 1; }
    }
}
