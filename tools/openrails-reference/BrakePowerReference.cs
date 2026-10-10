// Executes unmodified OR 1.6.1 subsystem classes. This harness supplies inputs;
// it contains no replacement brake/power equations and does not modify DLLs.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Serialization;
using System.Text;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using ORTS.Settings;
using Orts.Parsers.Msts;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;
using Orts.Simulation.RollingStocks.SubSystems.Brakes.MSTS;
using Orts.Simulation.RollingStocks.SubSystems.PowerSupplies;
using ORTS.Scripting.Api;

class BrakePowerReference
{
    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static void Set(object obj, string name, object value) {
        for(Type t = obj.GetType(); t != null; t = t.BaseType) {
            var p=t.GetProperty(name,BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly);
            if(p!=null) { p.SetValue(obj,value,null);return; }
            var f=t.GetField(name,BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly);
            if(f!=null) { f.SetValue(obj,value);return; }
        }
        throw new MissingMemberException(obj.GetType().Name,name);
    }
    static float Get(object obj, string name) {
        for(Type t=obj.GetType();t!=null;t=t.BaseType) {
            var f=t.GetField(name,BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly);
            if(f!=null)return Convert.ToSingle(f.GetValue(obj));
        }
        throw new MissingMemberException(obj.GetType().Name,name);
    }
    static float Quantity(string raw, STFReader.UNITS units) {
        using(var input=new MemoryStream(Encoding.UTF8.GetBytes("( "+raw+" )")))
        using(var reader=new STFReader(input,"brake-power-quantity",Encoding.UTF8,false))
            return reader.ReadFloatBlock(units,null);
    }
    static Simulator Simulator() {
        var sim=Empty<Simulator>();
        Set(sim,"Settings",new UserSettings(new[]{"AdhesionFactor=100","SimpleControlPhysics=true","UseAdvancedAdhesion=false"}));
        return sim;
    }
    static List<object> Vacuum(string input) {
        var results=new List<object>();
        foreach(JObject profile in JArray.Parse(File.ReadAllText(input))) {
            var fields=(JObject)profile["fields"];
            string system=(string)fields["BrakeSystemType"];
            if(system==null || !system.ToLowerInvariant().Contains("vacuum_"))continue;
            var sim=Simulator();
            var car=Empty<MSTSWagon>();Set(car,"Simulator",sim);
            car.CarLengthM=20f;car.MaxBrakeForceN=fields["MaxBrakeForce"]==null?100000:Quantity((string)fields["MaxBrakeForce"],STFReader.UNITS.Force);
            car.WagonType=MSTSWagon.WagonTypes.Freight;
            var lead=Empty<MSTSLocomotive>();Set(lead,"Simulator",sim);
            var train=Empty<Train>();train.Cars=new List<TrainCar>{lead,car};train.LeadLocomotiveIndex=0;
            car.Train=train;lead.Train=train;
            var brake=new VacuumSinglePipe(car);car.BrakeSystem=brake;
            foreach(var field in fields.Properties()) {
                using(var stream=new MemoryStream(Encoding.UTF8.GetBytes("( "+(string)field.Value+" )")))
                using(var reader=new STFReader(stream,"numeric-profile",Encoding.UTF8,false))
                    brake.Parse("wagon("+field.Name.ToLowerInvariant(),reader);
            }
            brake.Initialize();
            float max=fields["TrainBrakesControllerMaxSystemPressure"]==null?21f:
                Quantity((string)fields["TrainBrakesControllerMaxSystemPressure"],STFReader.UNITS.PressureDefaultPSI)/Quantity("1InHg",STFReader.UNITS.PressureDefaultPSI);
            brake.Initialize(false,max,max,true);
            var points=new List<object>();
            for(int i=0;i<1600;i++) {
                float command=i<100?0:i<400?.6f:i<600?0:i<800?1:0;
                bool bleed=i>=1100&&i<1140;
                brake.BrakeLine1PressurePSI=ORTS.Common.Vac.ToPress(max*(1-command));
                brake.BleedOffValveOpen=bleed;
                brake.Update(.05f);
                if(i%25==24)points.Add(new { tick=i+1, command=command, bleed=bleed,
                    pipe_psi=brake.BrakeLine1PressurePSI, cylinder_psi=Get(brake,"CylPressurePSIA"),
                    reservoir_psi=Get(brake,"VacResPressurePSIA"), adjusted_reservoir_psi=brake.GetVacResPressurePSI(),
                    shoe_force_n=car.BrakeShoeForceN, cylinder_vacuum_psi=brake.GetCylPressurePSI() });
            }
            results.Add(new { source=(string)profile["source"], file=(string)profile["file"],
                max_vacuum_inhg=max, atmosphere_psi=Quantity("1bar",STFReader.UNITS.PressureDefaultPSI),
                max_force_psi=Get(brake,"MaxForcePressurePSI"), max_force_n=car.MaxBrakeForceN,
                apply_psi_s=Get(brake,"MaxApplicationRatePSIpS"), release_psi_s=Get(brake,"MaxReleaseRatePSIpS"),
                cylinder_volume_m3=brake.GetTotalCylVolumeM3(), reservoir_volume_m3=brake.GetVacResVolume(),
                pipe_volume_m3=brake.BrakePipeVolumeM3, points=points });
        }
        return results;
    }
    static List<object> LegacyEp(string input) {
        var results=new List<object>();
        foreach(JObject profile in JArray.Parse(File.ReadAllText(input))) {
            var fields=(JObject)profile["fields"];
            string system=((string)fields["BrakeSystemType"]??"").Trim('"');
            if(!system.Equals("EP",StringComparison.OrdinalIgnoreCase) || fields["ORTSBrakeCylinderDiameter"]!=null)continue;
            var sim=Simulator();var car=Empty<MSTSWagon>();Set(car,"Simulator",sim);
            car.CarLengthM=20;car.MaxBrakeForceN=fields["MaxBrakeForce"]==null?100000:Quantity((string)fields["MaxBrakeForce"],STFReader.UNITS.Force);
            car.WagonType=MSTSWagon.WagonTypes.Passenger;
            var lead=Empty<MSTSLocomotive>();Set(lead,"Simulator",sim);
            var train=Empty<Train>();train.Cars=new List<TrainCar>{lead,car};train.LeadLocomotiveIndex=0;
            car.Train=train;lead.Train=train;lead.BrakeSystem=new EPBrakeSystem(lead);
            var brake=new EPBrakeSystem(car);car.BrakeSystem=brake;
            brake.SetBrakeEquipment(((string)fields["BrakeEquipmentType"]??"").Trim('"').ToLowerInvariant().Split(',').Select(s=>s.Trim()).ToList());
            foreach(var field in fields.Properties()) {
                using(var stream=new MemoryStream(Encoding.UTF8.GetBytes("( "+(string)field.Value+" )")))
                using(var reader=new STFReader(stream,"numeric-profile",Encoding.UTF8,false))
                    brake.Parse("wagon("+field.Name.ToLowerInvariant(),reader);
            }
            brake.Initialize();
            float charged=fields["TrainBrakesControllerMaxSystemPressure"]==null?90:Quantity((string)fields["TrainBrakesControllerMaxSystemPressure"],STFReader.UNITS.PressureDefaultPSI);
            train.BrakeLine4=0;brake.BrakeLine2PressurePSI=120;brake.Initialize(false,charged,charged-26,true);
            var points=new List<object>();
            for(int i=0;i<1600;i++) {
                float command=i<100?0:i<400?.6f:i<600?0:i<800?1:0;
                train.BrakeLine4=command;brake.BrakeLine1PressurePSI=charged;brake.BrakeLine2PressurePSI=120;
                brake.Update(.05f);
                if(i%25==24)points.Add(new {tick=i+1,command=command,pressure_psi=brake.GetCylPressurePSI(),auto_psi=brake.AutoCylPressurePSI,auxiliary_psi=Get(brake,"AuxResPressurePSI"),shoe_force_n=car.BrakeShoeForceN});
            }
            results.Add(new {source=(string)profile["source"],file=(string)profile["file"],charged_psi=charged,max_force_n=car.MaxBrakeForceN,aux_cylinder_ratio=Get(brake,"AuxCylVolumeRatio"),pipe_ratio=Get(brake,"AuxBrakeLineVolumeRatio"),application_psi_s=Get(brake,"ServiceApplicationRatePSIpS"),charging_psi_s=Get(brake,"MaxAuxilaryChargingRatePSIpS"),release_psi_s=Get(brake,"ReleaseRatePSIpS"),max_psi=Get(brake,"ServiceMaxCylPressurePSI"),reference_psi=Get(brake,"ReferencePressurePSI"),points=points});
        }
        return results;
    }
    static List<object> Power() {
        var rows=new List<object>();
        foreach(string kind in new[]{"electric","diesel","steam"}) {
            var sim=Simulator();
            sim.TRK=Empty<Orts.Formats.Msts.RouteFile>();sim.TRK.Tr_RouteFile=Empty<Orts.Formats.Msts.Tr_RouteFile>();
            sim.TRK.Tr_RouteFile.MaxLineVoltage=25000;
            MSTSLocomotive loco;
            ScriptedLocomotivePowerSupply host;
            LocomotivePowerSupply script;
            Pantograph pantograph=null;DieselEngine engine=null;
            if(kind=="electric") {
                var e=Empty<MSTSElectricLocomotive>();Set(e,"Simulator",sim);
                e.Pantographs=new Pantographs(e);pantograph=Empty<Pantograph>();e.Pantographs.List.Add(pantograph);
                host=new ScriptedElectricPowerSupply(e);script=new DefaultElectricPowerSupply();loco=e;
            } else if(kind=="diesel") {
                var d=Empty<MSTSDieselLocomotive>();Set(d,"Simulator",sim);
                d.DieselEngines=new DieselEngines(d);engine=new DieselEngine(d);d.DieselEngines.Add(engine);
                host=new ScriptedDieselPowerSupply(d);script=new DefaultDieselPowerSupply();loco=d;
            } else {
                var s=Empty<MSTSSteamLocomotive>();Set(s,"Simulator",sim);
                host=new ScriptedSteamPowerSupply(s);script=new DefaultSteamPowerSupply();loco=s;
            }
            loco.PowerSupply=host;Set(script,"Host",host);Set(script,"Car",loco);
            Set(host,"PowerOnDelayS",2f);Set(host,"AuxPowerOnDelayS",1f);
            Set(host.ElectricTrainSupplySwitch,"Mode",ElectricTrainSupplySwitch.ModeType.Unfitted);
            float clock=0;script.GameTime=()=>clock;script.ClockTime=()=>clock;
            script.SignalEvent=evt=>{};script.SignalEventToTrain=evt=>{};
            script.Initialize();
            for(int i=0;i<=100;i++) {
                clock=i*.1f;
                bool battery=i<30||i>=40, master=i<45||i>=50, source=i<55||i>=65, contact=i<75||i>=80;
                Set(host.BatterySwitch,"On",battery);Set(host.MasterKey,"On",master);
                if(pantograph!=null) {
                    Set(pantograph,"State",source?PantographState.Up:PantographState.Down);
                    Set(((ScriptedElectricPowerSupply)host).CircuitBreaker,"State",contact?CircuitBreakerState.Closed:CircuitBreakerState.Open);
                }
                if(engine!=null) {
                    Set(engine,"State",source?DieselEngineState.Running:DieselEngineState.Stopped);
                    Set(((ScriptedDieselPowerSupply)host).TractionCutOffRelay,"State",contact?TractionCutOffRelayState.Closed:TractionCutOffRelayState.Open);
                }
                script.Update(i==0?0f:.1f);
                rows.Add(new { kind=kind, tick=i, time_s=clock, battery=battery, master=master,
                    source=source, contact=contact, main=host.MainPowerSupplyOn, auxiliary=host.AuxiliaryPowerSupplyOn,
                    low_voltage=host.LowVoltagePowerSupplyOn, cab=host.CabPowerSupplyOn });
            }
        }
        return rows;
    }
    static void Main(string[] args) {
        if(args.Length!=2)throw new ArgumentException("BrakePowerReference PROFILES.json OUTPUT.json");
        File.WriteAllText(args[1],JsonConvert.SerializeObject(new {
            reference="Open Rails 1.6.1", dt_s=.05, vacuum=Vacuum(args[0]), legacy_ep=LegacyEp(args[0]), power=Power(),
            scope="Original automatic vacuum valves/reservoirs and default electric/diesel/steam main, auxiliary, battery and cab power; prescribed subsystem inputs"
        },Formatting.Indented).Replace("\r\n","\n")+"\n");
    }
}
