// Calls unmodified Open Rails 1.6.1 DieselEngine.Update, STF units and sound
// event mapping. This is not a complete steam-thermodynamics reference.
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.Serialization;
using System.Text;
using Newtonsoft.Json;
using Orts.Parsers.Msts;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;
using Orts.Simulation.RollingStocks.SubSystems.PowerSupplies;
using ORTS.Scripting.Api;

class TractionOperationReference
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
    static float Quantity(string raw, STFReader.UNITS units) {
        using (var input = new MemoryStream(Encoding.UTF8.GetBytes("( " + raw + " )")))
        using (var reader = new STFReader(input, "traction-operation-units", Encoding.UTF8, false))
            return reader.ReadFloatBlock(units, null);
    }
    static void Main(string[] args) {
        if (args.Length != 1) throw new ArgumentException("TractionOperationReference OUTPUT.json");
        var loco = Empty<MSTSDieselLocomotive>();
        Set(loco, "Simulator", Empty<Simulator>());
        loco.Train = Empty<Train>();
        loco.Train.Cars = new List<TrainCar>();
        loco.RemoteControlGroup = -1;
        var supply = new ScriptedDieselPowerSupply(loco);
        loco.PowerSupply = supply;
        supply.MainPowerSupplyState = PowerSupplyState.PowerOn;
        supply.MaxThrottlePercent = 100;
        loco.DieselTransmissionType = MSTSDieselLocomotive.DieselTransmissionTypes.Electric;
        loco.DieselEngines = new DieselEngines(loco);
        var engine = new DieselEngine(loco);
        loco.DieselEngines.Add(engine);
        engine.IdleRPM = 300;
        engine.MaxRPM = 900;
        engine.StartingRPM = 200;
        engine.StartingConfirmationRPM = 330;
        engine.ChangeUpRPMpS = engine.ChangeDownRPMpS = 80;
        engine.RateOfChangeUpRPMpSS = engine.RateOfChangeDownRPMpSS = 80;
        engine.MaximumDieselPowerW = 1000000;
        engine.DieselTempTimeConstantSec = 100;
        engine.ThrottleRPMTab = new Interpolator(new float[] { 0, 100 }, new float[] { 300, 900 });
        engine.ReverseThrottleRPMTab = new Interpolator(new float[] { 300, 900 }, new float[] { 0, 100 });
        engine.DieselPowerTab = new Interpolator(new float[] { 0, 300, 900 }, new float[] { 0, 20000, 1000000 });
        engine.DieselConsumptionTab = new Interpolator(new float[] { 0, 300, 900 }, new float[] { 0, 18, 180 });
        engine.RealRPM = engine.DemandedRPM = 300;
        Set(engine, "State", DieselEngineState.Running);
        var checkpoints = new List<object>();
        bool requested = true;
        double used = 0;
        for (int tick = 0; tick < 400; tick++) {
            if (tick == 20 || tick == 340) {
                requested = false;
                engine.HandleEvent(PowerSupplyEvent.StopEngine);
            }
            if (tick == 60) {
                requested = true;
                engine.HandleEvent(PowerSupplyEvent.StartEngine);
            }
            loco.LocalThrottlePercent = tick >= 180 && tick < 280 ? 60 : 0;
            engine.Update(.05f);
            used += engine.DieselFlowLps * .05;
            if (tick % 10 == 9)
                checkpoints.Add(new { tick = tick + 1, time_s = (tick+1)*.05,
                    throttle = loco.LocalThrottlePercent / 100f, command_running = requested,
                    state = engine.State.ToString(), rpm = engine.RealRPM,
                    demanded_rpm = engine.DemandedRPM, flow_lps = engine.DieselFlowLps,
                    consumed_l = used });
        }
        var quantities = new List<object> {
            new { key = "MaxBoilerPressure", raw = "225", value = (double)Quantity("225", STFReader.UNITS.PressureDefaultPSI) * .06894757293168, unit = "bar" },
            new { key = "BoilerVolume", raw = "\"225*(ft^3)\"", value = (double)Quantity("\"225*(ft^3)\"", STFReader.UNITS.VolumeDefaultFT3) * 28.316846592, unit = "litres" },
            new { key = "MaxTenderWaterMass", raw = "40000lb", value = (double)Quantity("40000lb", STFReader.UNITS.Mass), unit = "kg" },
            new { key = "MaxTenderCoalMass", raw = "13440lb", value = (double)Quantity("13440lb", STFReader.UNITS.Mass), unit = "kg" },
            new { key = "MaxDieselLevel", raw = "500gal", value = (double)Quantity("500gal", STFReader.UNITS.Volume), unit = "litres" },
            new { key = "MaxDieselLevel", raw = "500g-uk", value = (double)Quantity("500g-uk", STFReader.UNITS.Volume), unit = "litres" }
        };
        var events = new List<object>();
        foreach (int id in new int[] { 23, 24, 27, 28, 30, 31, 32, 33, 34, 137, 138 })
            events.Add(new { id = id, native_event = Orts.Common.Events.From(Orts.Common.Events.Source.MSTSCar, id).ToString() });
        File.WriteAllText(args[0], JsonConvert.SerializeObject(new {
            reference = "Open Rails 1.6.1", time_step_s = .05,
            idle_rpm = 300, max_rpm = 900, starting_rpm = 200, confirmation_rpm = 330, rpm_rate = 80,
            consumption_lph = new[] { new[] { 0, 0 }, new[] { 300, 18 }, new[] { 900, 180 } },
            checkpoints = checkpoints, quantities = quantities, sound_events = events,
            scope = "Native non-geared DieselEngine start/stop, RPM and flow; native resource units and sound IDs. No complete steam, gearbox, brake, power-supply or fuel-weight parity claim."
        }, Formatting.Indented).Replace("\r\n", "\n") + "\n");
    }
}
