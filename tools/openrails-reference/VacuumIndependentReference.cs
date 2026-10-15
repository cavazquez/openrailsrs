// Executes the unmodified, pinned OR 1.6.1 DLL. No vacuum/brake equations live
// in this helper: it supplies controls, calls propagation/update, and records.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Serialization;
using System.Text;
using System.Globalization;
using System.Threading;
using GNU.Gettext;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using ORTS.Settings;
using ORTS.Common;
using Orts.Parsers.Msts;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;
using Orts.Simulation.RollingStocks.SubSystems.Brakes.MSTS;
using Orts.Simulation.RollingStocks.SubSystems.Controllers;

class VacuumIndependentReference
{
    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static void Set(object obj, string name, object value) {
        for (Type t = obj.GetType(); t != null; t = t.BaseType) {
            var p = t.GetProperty(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if (p != null) { p.SetValue(obj, value, null); return; }
            var f = t.GetField(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if (f != null) { f.SetValue(obj, value); return; }
        }
        throw new MissingMemberException(obj.GetType().Name, name);
    }
    static float Get(object obj, string name) {
        for (Type t = obj.GetType(); t != null; t = t.BaseType) {
            var f = t.GetField(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly);
            if (f != null) return Convert.ToSingle(f.GetValue(obj));
        }
        throw new MissingMemberException(obj.GetType().Name, name);
    }
    static void Parse(Action<STFReader> action, string value) {
        using (var input = new MemoryStream(Encoding.UTF8.GetBytes("( " + value + " )")))
        using (var reader = new STFReader(input, "vacuum-independent-input", Encoding.UTF8, false)) action(reader);
    }
    static float Quantity(string value, STFReader.UNITS units) {
        float result = 0;
        Parse(reader => result = reader.ReadFloatBlock(units, null), value);
        return result;
    }
    static object Capture(JObject spec) {
        var simulator = Empty<Simulator>();
        Set(simulator, "Settings", new UserSettings(new[] {"AdhesionFactor=100", "SimpleControlPhysics=true", "UseAdvancedAdhesion=false"}));
        simulator.Confirmer = new Confirmer(simulator, 1);
        bool steam = (bool)spec["steam"];
        MSTSLocomotive lead = steam ? (MSTSLocomotive)Empty<MSTSSteamLocomotive>() : Empty<MSTSLocomotive>();
        Set(lead, "Simulator", simulator);
        lead.TrainBrakeController = new ScriptedBrakeController(lead);
        lead.EngineBrakeController = new ScriptedBrakeController(lead);
        foreach (var field in ((JObject)spec["controller_fields"]).Properties()) {
            var controller = field.Name == "Brake_Train" ? lead.TrainBrakeController : lead.EngineBrakeController;
            string token = field.Name.StartsWith("Brake_") ? "engine(enginecontrollers(" + field.Name.ToLowerInvariant() : "engine(" + field.Name.ToLowerInvariant();
            Parse(reader => controller.Parse(token, reader), (string)field.Value);
        }
        lead.TrainBrakeController.MaxPressurePSI = Quantity((string)spec["max_vacuum"], STFReader.UNITS.PressureDefaultPSI);
        lead.TrainBrakeController.Initialize();
        lead.EngineBrakeController.Initialize();
        lead.EngineBrakeFitted = true;
        lead.SteamEngineBrakeFitted = false;
        lead.CarHeightAboveSeaLevelM = 0;
        lead.BrakePipeTimeFactorS = .0015f;
        lead.IsDriveable = true;
        var train = Empty<Train>();
        train.Cars = new List<TrainCar>();
        train.LeadLocomotiveIndex = 0;
        train.WagonsAttached = true;
        var brakes = new List<VacuumSinglePipe>();
        foreach (JObject vehicle in (JArray)spec["vehicles"]) {
            var fields = (JObject)vehicle["fields"];
            var car = train.Cars.Count == 0 ? (MSTSWagon)lead : Empty<MSTSWagon>();
            Set(car, "Simulator", simulator);
            car.Train = train;
            car.CarLengthM = (float)vehicle["length_m"];
            car.MaxBrakeForceN = Quantity((string)fields["MaxBrakeForce"], STFReader.UNITS.Force);
            car.WagonType = train.Cars.Count == 0 ? TrainCar.WagonTypes.Engine :
                (bool)vehicle["tender"] ? TrainCar.WagonTypes.Tender : TrainCar.WagonTypes.Passenger;
            car.CarBrakeSystemType = "vacuum_single_pipe";
            var brake = new VacuumSinglePipe(car);
            car.BrakeSystem = brake;
            brake.FrontBrakeHoseConnected = true;
            brake.AngleCockAOpen = true;
            brake.AngleCockBOpen = true;
            foreach (var field in fields.Properties()) {
                Parse(reader => brake.Parse("wagon(" + field.Name.ToLowerInvariant(), reader), (string)field.Value);
            }
            brake.Initialize();
            train.Cars.Add(car);
            brakes.Add(brake);
        }
        float maxVacuum = Quantity((string)spec["max_vacuum"], STFReader.UNITS.PressureDefaultPSI);
        float maxInHg = maxVacuum / Quantity("1InHg", STFReader.UNITS.PressureDefaultPSI);
        foreach (var brake in brakes) {
            brake.Initialize(false, maxInHg, maxInHg, true);
            Set(brake, "VacResPressurePSIA", (float)spec["initial_reservoir_psi"]);
        }
        var initial = brakes.Select((b, i) => new { index = i,
            pipe_psi = b.BrakeLine1PressurePSI, independent_pipe_psi = b.BrakeLine3PressurePSI,
            cylinder_psi = Get(b, "CylPressurePSIA"), reservoir_psi = Get(b, "VacResPressurePSIA"),
            brake_cyl_fraction = b.BrakeCylFraction }).ToArray();
        var points = new List<object>();
        int tick = 0;
        const float dt = .05f;
        foreach (JObject phase in (JArray)spec["phases"]) {
            float trainCommand = (float)phase["train"];
            float independentCommand = (float)phase["independent"];
            bool vented = (bool?)phase["vented"] ?? false;
            float pipeInput = (string)phase["pipe_input"] == "charged" ? Vac.ToPress(maxInHg) :
                Quantity((string)phase["pipe_input"], STFReader.UNITS.PressureDefaultPSI);
            for (int step = 0; step < (int)phase["ticks"]; step++) {
                lead.TrainBrakeController.SetValue(trainCommand);
                lead.EngineBrakeController.SetValue(independentCommand);
                lead.TrainBrakeController.Update(dt);
                lead.EngineBrakeController.Update(dt);
                // Vac.FromPress returns inHg. The native propagation method
                // expects vacuum PSI in this historically mixed-unit field.
                train.EqualReservoirPressurePSIorInHg = Vac.FromPress(pipeInput) *
                    Quantity("1InHg", STFReader.UNITS.PressureDefaultPSI);
                foreach (var brake in brakes) {
                    brake.BrakeLine1PressurePSI = pipeInput;
                    brake.FrontBrakeHoseConnected = true;
                    brake.AngleCockAOpen = true;
                    brake.AngleCockBOpen = true;
                }
                brakes[0].AngleCockAOpen = false;
                brakes[brakes.Count - 1].AngleCockBOpen = false;
                // Full real native propagation, including pipe 3. The actual
                // post-propagation pipe 1 is recorded as the Rust valve input.
                lead.BrakeSystem.PropagateBrakePressure(dt);
                var inputs = brakes.Select(b => b.BrakeLine1PressurePSI).ToArray();
                foreach (var brake in brakes) brake.Update(dt);
                tick++;
                points.Add(new { tick, phase = (string)phase["name"], train_command = lead.TrainBrakeController.CurrentValue,
                    independent_command = lead.EngineBrakeController.CurrentValue,
                    controller_state = lead.EngineBrakeController.TrainBrakeControllerState.ToString(), vented,
                    vehicles = brakes.Select((b, i) => new { index = i, pipe_input_psi = inputs[i],
                        pipe_psi = b.BrakeLine1PressurePSI, independent_pipe_psi = b.BrakeLine3PressurePSI,
                        cylinder_psi = Get(b, "CylPressurePSIA"), reservoir_psi = Get(b, "VacResPressurePSIA"),
                        brake_cyl_fraction = b.BrakeCylFraction,
                        adjusted_reservoir_psi = b.GetVacResPressurePSI(), cylinder_vacuum_psi = b.GetCylPressurePSI(),
                        shoe_force_n = train.Cars[i].BrakeShoeForceN }).ToArray() });
            }
        }
        return new { name = (string)spec["name"], max_vacuum_psi = maxVacuum, dt_s = dt,
            controller = new { application_psi_s = lead.EngineBrakeController.ApplyRatePSIpS,
                release_psi_s = lead.EngineBrakeController.ReleaseRatePSIpS,
                emergency_psi_s = lead.EngineBrakeController.EmergencyRatePSIpS },
            hardware = brakes.Select((b, i) => new { index = i,
                max_force_psi = Get(b, "MaxForcePressurePSI"), max_force_n = train.Cars[i].MaxBrakeForceN,
                application_psi_s = Get(b, "MaxApplicationRatePSIpS"), release_psi_s = Get(b, "MaxReleaseRatePSIpS"),
                cylinder_volume_m3 = b.GetTotalCylVolumeM3(), reservoir_volume_m3 = b.GetVacResVolume(),
                pipe_volume_m3 = b.BrakePipeVolumeM3 }).ToArray(),
            methods = new[] {"ScriptedBrakeController.Parse/Initialize/SetValue/Update", "VacuumSinglePipe.Initialize", "VacuumSinglePipe.PropagateBrakePressure", "VacuumSinglePipe.Update", "VacuumSinglePipe.GetVacResPressurePSI"},
            initial, points };
    }
    static int Main(string[] args) {
        try {
            if (args.Length != 2) throw new ArgumentException("VacuumIndependentReference INPUT.json OUTPUT.json");
            Thread.CurrentThread.CurrentCulture = CultureInfo.InvariantCulture;
            Thread.CurrentThread.CurrentUICulture = CultureInfo.InvariantCulture;
            // Same catalog construction as Simulator's constructor; the
            // subsystem fixture does not start a route or viewer.
            typeof(Simulator).GetProperty("Catalog", BindingFlags.Public | BindingFlags.Static)
                .SetValue(null, new GettextResourceManager("Orts.Simulation"), null);
            var config = JObject.Parse(File.ReadAllText(args[0]));
            var cases = ((JArray)config["cases"]).Cast<JObject>().Select(Capture).ToArray();
            File.WriteAllText(args[1], JsonConvert.SerializeObject(new { schema = 1, reference = "Open Rails 1.6.1", cases }, Formatting.Indented));
            return 0;
        } catch (Exception error) {
            Console.Error.WriteLine(error);
            return 1;
        }
    }
}
