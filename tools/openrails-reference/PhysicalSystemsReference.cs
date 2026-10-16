// Original DLL calls only. No thermodynamic, valve or traction equations are
// implemented here. All prescribed inputs are emitted alongside their output.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Serialization;
using System.Text;
using System.Threading;
using GNU.Gettext;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using ORTS.Settings;
using ORTS.Common;
using ORTS.Scripting.Api;
using Orts.Parsers.Msts;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;
using Orts.Simulation.RollingStocks.SubSystems.Brakes.MSTS;
using Orts.Simulation.RollingStocks.SubSystems.Controllers;

class PhysicalSystemsReference {
    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static object Get(object o,string name) {
        for(Type t=o.GetType();t!=null;t=t.BaseType) {
            var f=t.GetField(name,BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly);
            if(f!=null)return f.GetValue(o);
            var p=t.GetProperty(name,BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly);
            if(p!=null)return p.GetValue(o,null);
        }
        throw new MissingMemberException(o.GetType().Name,name);
    }
    static void Set(object o,string name,object v) {
        for(Type t=o.GetType();t!=null;t=t.BaseType) {
            var f=t.GetField(name,BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly);
            if(f!=null){f.SetValue(o,v);return;}
            var p=t.GetProperty(name,BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly);
            if(p!=null){p.SetValue(o,v,null);return;}
        }
        throw new MissingMemberException(o.GetType().Name,name);
    }
    static float F(object o,string n){return Convert.ToSingle(Get(o,n));}
    static void Json(string path,object o){File.WriteAllText(path,JsonConvert.SerializeObject(o,Formatting.Indented).Replace("\r\n","\n")+"\n");}
    static void Parse(Action<STFReader> a,string value) {
        using(var s=new MemoryStream(Encoding.UTF8.GetBytes("( "+value+" )")))
        using(var r=new STFReader(s,"physical-systems-input",Encoding.UTF8,false))a(r);
    }
    static float Quantity(string s,STFReader.UNITS units){float v=0;Parse(r=>v=r.ReadFloatBlock(units,null),s);return v;}
    static UserSettings Settings(){return new UserSettings(new[]{"Language=en","SimpleControlPhysics=true",
        "UseAdvancedAdhesion=false","CorrectQuestionableBrakingParams=false","AdhesionFactor=100",
        "AdhesionFactorChange=0","AdhesionProportionalToWeather=false","CurveSpeedDependent=false",
        "TunnelResistanceDependent=false","WindResistanceDependent=false","Alerter=false","HotStart=true",
        "ActRandomizationLevel=0","ActWeatherRandomizationLevel=0","NoForcedRedAtStationStops=true"});}
    static object Properties(JObject input) {
        var type=typeof(Simulator).Assembly.GetType("Orts.Common.SteamTable",true);
        string[] methods={"PressureToTemperatureInterpolatorPSItoF","WaterHeatInterpolatorPSItoBTUpLB",
            "WaterDensityInterpolatorPSItoLBpFT3","SteamHeatInterpolatorPSItoBTUpLB","SteamDensityInterpolatorPSItoLBpFT3"};
        var tables=methods.Select(n=>(Interpolator)type.GetMethod(n,BindingFlags.Static|BindingFlags.Public).Invoke(null,null)).ToArray();
        return ((JArray)input["pressure_psi"]).Select(v=> {
            float p=(float)v;return new {pressure_psi=p,temperature_f=tables[0][p],water_heat_btu_lb=tables[1][p],
                water_density_lb_ft3=tables[2][p],steam_heat_btu_lb=tables[3][p],steam_density_lb_ft3=tables[4][p]};
        }).ToArray();
    }
    static object Brakes(JObject input) {
        var result=new List<object>();
        foreach(JObject profile in (JArray)input["brake_profiles"]) {
            var simulator=Empty<Simulator>();Set(simulator,"Settings",Settings());
            simulator.Confirmer=new Confirmer(simulator,1);
            var lead=Empty<MSTSLocomotive>();Set(lead,"Simulator",simulator);
            lead.TrainBrakeController=new ScriptedBrakeController(lead);
            lead.EngineBrakeController=new ScriptedBrakeController(lead);
            var train=Empty<Train>();train.Cars=new List<TrainCar>();train.LeadLocomotiveIndex=0;train.WagonsAttached=true;
            lead.Train=train;lead.BrakeSystem=new AirSinglePipe(lead);train.Cars.Add(lead);
            var car=Empty<MSTSWagon>();Set(car,"Simulator",simulator);car.Train=train;car.CarLengthM=(float)profile["length_m"];
            car.MaxBrakeForceN=Quantity((string)profile["max_force"],STFReader.UNITS.Force);car.WagonType=TrainCar.WagonTypes.Freight;train.Cars.Add(car);
            var air=new AirSinglePipe(car);car.BrakeSystem=air;
            var fields=(JObject)profile["fields"];
            air.SetBrakeEquipment(((string)fields["BrakeEquipmentType"]).Trim('"').ToLowerInvariant().Split(',').Select(s=>s.Trim()).ToList());
            foreach(var field in fields.Properties()) if(!field.Name.StartsWith("ORTSRS",StringComparison.OrdinalIgnoreCase))
                Parse(r=>air.Parse("wagon("+field.Name.ToLowerInvariant(),r),(string)field.Value);
            air.Initialize();float charged=Quantity((string)profile["charged_pressure"],STFReader.UNITS.PressureDefaultPSI);air.Initialize(false,charged,charged,true);
            var initial=new {pipe_psi=air.BrakeLine1PressurePSI,cylinder_psi=air.GetCylPressurePSI(),
                auxiliary_psi=F(air,"AuxResPressurePSI"),emergency_reservoir_psi=F(air,"EmergResPressurePSI"),
                control_psi=F(air,"ControlResPressurePSI"),valve=Get(air,"TripleValveState").ToString()};
            var points=new List<object>();int tick=0;
            foreach(JObject phase in (JArray)input["brake_phases"])for(int k=0;k<(int)phase["ticks"];k++) {
                float external=(float)phase["pipe_psi"]+(phase["pipe_delta_per_tick"]==null?0:(float)phase["pipe_delta_per_tick"]*k),
                    independent=(float)phase["independent_psi"],dt=(float)input["dt_s"];
                air.BrakeLine1PressurePSI=external;air.BrakeLine3PressurePSI=independent;
                Set(simulator,"GameTime",(double)(tick*dt));air.Update(dt);tick++;
                points.Add(new{tick=tick,phase=(string)phase["id"],dt_s=dt,input_pipe_psi=external,
                    input_independent_psi=independent,pipe_psi=air.BrakeLine1PressurePSI,
                    cylinder_psi=air.GetCylPressurePSI(),auxiliary_psi=F(air,"AuxResPressurePSI"),
                    emergency_reservoir_psi=F(air,"EmergResPressurePSI"),control_psi=F(air,"ControlResPressurePSI"),
                    valve=Get(air,"TripleValveState").ToString(),shoe_force_n=car.BrakeShoeForceN});
            }
            result.Add(new{id=(string)profile["id"],max_force_n=car.MaxBrakeForceN,charged_psi=charged,initial=initial,parameters=new {
                aux_volume_m3=F(air,"AuxResVolumeM3"),emergency_volume_m3=F(air,"EmergResVolumeM3"),
                cylinder_volume_m3=air.GetTotalCylVolumeM3(),pipe_volume_m3=air.BrakePipeVolumeM3,
                triple_ratio=F(air,"AuxCylVolumeRatio"),max_cylinder_psi=F(air,"MaxCylPressurePSI"),
                reference_psi=F(air,"ReferencePressurePSI"),actuation_psi_s=F(air,"EmergencyValveActuationRatePSIpS")},points=points});
        }
        return result;
    }
    static object AirSample(MSTSWagon car) {
        var a=(AirSinglePipe)car.BrakeSystem;
        return new {pipe_psi=a.BrakeLine1PressurePSI,cylinder_psi=a.GetCylPressurePSI(),
            auxiliary_psi=F(a,"AuxResPressurePSI"),emergency_reservoir_psi=F(a,"EmergResPressurePSI"),
            control_psi=F(a,"ControlResPressurePSI"),valve=Get(a,"TripleValveState").ToString(),
            shoe_force_n=car.BrakeShoeForceN};
    }
    static object BrakeNetworks(JObject input) {
        var result=new List<object>();
        foreach(JObject profile in (JArray)input["brake_profiles"]) {
            var simulator=Empty<Simulator>();Set(simulator,"Settings",Settings());
            simulator.Confirmer=new Confirmer(simulator,1);
            var train=Empty<Train>();Set(train,"Simulator",simulator);train.Cars=new List<TrainCar>();
            train.LeadLocomotiveIndex=0;train.DPLeadUnits=new List<TrainCar>();train.LocoGroups=new List<List<TrainCar>>();
            // OR disables gradient-triggered emergency valves on a non-static
            // train without a leading locomotive. Supply that native context,
            // with both cocks closed so it exchanges no air with the three
            // measured freight cars. No main-reservoir/feed-valve is advanced.
            var lead=Empty<MSTSLocomotive>();Set(lead,"Simulator",simulator);lead.Train=train;lead.CarLengthM=20;
            lead.TrainBrakeController=new ScriptedBrakeController(lead);lead.EngineBrakeController=new ScriptedBrakeController(lead);
            lead.BrakeSystem=new AirSinglePipe(lead);lead.BrakeSystem.FrontBrakeHoseConnected=false;
            lead.BrakeSystem.AngleCockAOpen=false;lead.BrakeSystem.AngleCockBOpen=false;
            lead.BrakeSystem.AngleCockAOpenAmount=0;lead.BrakeSystem.AngleCockBOpenAmount=0;train.Cars.Add(lead);
            var cars=new List<MSTSWagon>();var hardware=new List<object>();
            var fields=(JObject)profile["fields"];
            float charged=Quantity((string)profile["charged_pressure"],STFReader.UNITS.PressureDefaultPSI);
            for(int i=0;i<3;i++) {
                var car=Empty<MSTSWagon>();Set(car,"Simulator",simulator);car.Train=train;car.CarLengthM=20+10*i;
                car.MaxBrakeForceN=Quantity((string)profile["max_force"],STFReader.UNITS.Force);car.WagonType=TrainCar.WagonTypes.Freight;
                train.Cars.Add(car);cars.Add(car);var air=new AirSinglePipe(car);car.BrakeSystem=air;
                air.SetBrakeEquipment(((string)fields["BrakeEquipmentType"]).Trim('"').ToLowerInvariant().Split(',').Select(s=>s.Trim()).ToList());
                foreach(var field in fields.Properties()) if(!field.Name.StartsWith("ORTSRS",StringComparison.OrdinalIgnoreCase))
                    Parse(r=>air.Parse("wagon("+field.Name.ToLowerInvariant(),r),(string)field.Value);
                air.Initialize();air.Initialize(false,charged,charged,true);
                air.FrontBrakeHoseConnected=i>0;air.AngleCockAOpen=i>0;air.AngleCockBOpen=i<2;
                air.AngleCockAOpenAmount=i>0?1:0;air.AngleCockBOpenAmount=i<2?1:0;
                hardware.Add(new {length_m=car.CarLengthM,pipe_volume_m3=air.BrakePipeVolumeM3,max_force_n=car.MaxBrakeForceN});
            }
            var initial=cars.Select(c=>AirSample(c)).ToArray();var points=new List<object>();int tick=0;
            foreach(JObject phase in (JArray)input["brake_phases"])for(int k=0;k<(int)phase["ticks"];k++) {
                float head=(float)phase["pipe_psi"]+(phase["pipe_delta_per_tick"]==null?0:(float)phase["pipe_delta_per_tick"]*k);
                float dt=(float)input["dt_s"];cars[0].BrakeSystem.BrakeLine1PressurePSI=head;
                Set(simulator,"GameTime",(double)(tick*dt));cars[0].BrakeSystem.PropagateBrakePressure(dt);
                foreach(var car in cars)((AirSinglePipe)car.BrakeSystem).Update(dt);tick++;
                points.Add(new {tick=tick,phase=(string)phase["id"],dt_s=dt,input_head_pipe_psi=head,
                    cars=cars.Select(c=>AirSample(c)).ToArray()});
            }
            result.Add(new {id=(string)profile["id"],hardware=hardware,initial=initial,points=points,
                methods=new[]{"AirSinglePipe.PropagateBrakePressure","AirSinglePipe.Update"},
                scope="Three freight-car subsystems, only head pipe1 prescribed once per tick. A locomotive isolated by closed cocks supplies the native gradient-enabled context without exchanging air. No feed valve, pipe3 demand, speed or forces prescribed."});
        }
        return result;
    }
    static object SteamSample(MSTSSteamLocomotive l,int tick) {
        return new {tick=tick,speed_mps=l.Train.SpeedMpS,distance_m=l.Train.DistanceTravelledM,
            pressure_psi=F(l,"BoilerPressurePSI"),boiler_mass_lb=l.BoilerMassLB,boiler_heat_btu=F(l,"BoilerHeatBTU"),
            water_fraction=F(l,"WaterFraction"),evaporation_lb_s=l.EvaporationLBpS,steam_usage_lb_s=l.PreviousTotalSteamUsageLBpS,
            water_consumption_lb_s=l.WaterConsumptionLbpS,heat_in_btu_s=l.BoilerHeatInBTUpS,
            heat_out_btu_s=l.PreviousBoilerHeatOutBTUpS,motive_force_n=l.MotiveForceN,
            fire_mass_kg=F(l,"FireMassKG"),tender_coal_kg=l.TenderFuelMassKG,
            tender_water_ukg=l.CurrentLocoTenderWaterVolumeUKG,
            combined_tender_water_ukg=l.CombinedTenderWaterVolumeUKG,water_lb_per_ukg=l.WaterLBpUKG,
            boiler_kw=F(l,"BoilerKW"),flue_temp_k=F(l,"FlueTempK"),
            boiler_water_temp_k=F(l,"BoilerWaterTempK"),water_temp_new_k=F(l,"WaterTempNewK"),
            boiler_water_density_lb_ft3=F(l,"BoilerWaterDensityLBpFT3"),
            boiler_steam_density_lb_ft3=F(l,"BoilerSteamDensityLBpFT3"),
            injector1=l.Injector1IsOn,injector2=l.Injector2IsOn,
            injector1_fraction=F(l,"Injector1Fraction"),injector2_fraction=F(l,"Injector2Fraction"),
            injector_water_input_lb=F(l,"BoilerWaterInputLB"),
            injector_nominal_flow_lb_s=F(l,"InjectorFlowRateLBpS"),
            fire_burn_kg_s=l.FuelBurnRateSmoothedKGpS,fuel_feed_kg_s=F(l,"FuelFeedRateKGpS"),
            cylinder_usage_lb_s=l.CylinderSteamUsageLBpS,safety_usage_lb_s=l.SafetyValveUsageLBpS,
            regulator=l.ThrottlePercent*.01f,cutoff=l.CutoffController.CurrentValue,
            damper=l.DamperController.CurrentValue,firing=l.FiringRateController.CurrentValue};
    }
    static void SteamDrive(string path,string consist,string output,JObject config) {
        Thread.CurrentThread.Name="Loader Process";var simulator=new Simulator(Settings(),path,false,true);
        simulator.Confirmer=new Confirmer(simulator,1);simulator.SetExplore(path,consist,"9:29","1","0");
        simulator.Start(new ORTS.Common.CancellationTokenSource(delegate{}).Token);
        simulator.SetCommandReceivers();simulator.PreUpdate=false;Directory.CreateDirectory(output);
        Exception failure=null;
        var worker=new Thread(delegate(){try{
            var l=simulator.PlayerLocomotive as MSTSSteamLocomotive;
            if(l==null)throw new InvalidOperationException("Expected an original steam locomotive");
            Set(l,"FiringIsManual",true);
            l.SetDirection(Direction.Forward);var commands=(JArray)config["commands"];int command=0;
            var parameters=new {boiler_volume_ft3=F(l,"BoilerVolumeFT3"),
                max_pressure_psi=F(l,"MaxBoilerPressurePSI"),max_fire_mass_kg=F(l,"MaxFireMassKG"),
                max_firing_rate_kg_s=F(l,"MaxFiringRateKGpS"),
                max_fuel_burn_grate_kg_s=F(l,"MaxFuelBurnGrateKGpS"),
                max_steam_output_lb_s=F(l,"TheoreticalMaxSteamOutputLBpS"),
                coal_heat_kj_kg=F(l,"FuelCalorificKJpKG"),
                evaporation_area_m2=F(l,"EvaporationAreaM2"),
                heat_transfer_w_m2_k=F(l,"BoilerHeatTransferCoeffWpM2K"),
                heat_material_thickness_factor=F(l,"HeatMaterialThicknessFactor"),
                hp_cylinders=l.MSTSNumCylinders,hp_diameter_m=l.MSTSCylinderDiameterM,
                hp_stroke_m=l.MSTSCylinderStrokeM,has_tender_coupled=l.HasTenderCoupled,
                settings_simple_control_physics=true,settings_use_advanced_adhesion=false,
                settings_hot_start=simulator.Settings.HotStart,firing_is_manual=true};
            var rows=new List<object>();rows.Add(SteamSample(l,0));
            for(int tick=0;tick<(int)config["ticks"];tick++) {
                while(command+1<commands.Count&&(int)commands[command+1]["tick"]<=tick)command++;
                var c=commands[command];l.SetThrottlePercentWithSound((float)c["throttle"]*100);
                l.SetTrainBrakePercent((float)c["brake"]*100);
                l.CutoffController.SetValue((float)c["cutoff"]);l.CutoffController.Update(.05f);
                l.DamperController.SetValue((float)c["damper"]);l.DamperController.Update(.05f);
                l.FiringRateController.SetValue((float)c["firing"]);l.FiringRateController.Update(.05f);
                l.Injector1Controller.SetValue((bool)c["injector1"]?1:0);l.Injector1Controller.Update(.05f);
                l.Injector2Controller.SetValue((bool)c["injector2"]?1:0);l.Injector2Controller.Update(.05f);
                l.Injector1IsOn=(bool)c["injector1"];l.Injector2IsOn=(bool)c["injector2"];
                simulator.Update(.05f);rows.Add(SteamSample(l,tick+1));
            }
            Json(Path.Combine(output,"steam-drive.json"),new {method="Simulator.Update",input=config,parameters=parameters,rows=rows,
                scope="Original locomotive/routing reference; no substituted speeds or steam equations. Full Rust steam-cycle parity is not implied."});
        }catch(Exception e){failure=e;}});worker.Name="Updater Process";worker.Start();worker.Join();
        if(failure!=null)throw new InvalidOperationException("Original steam drive failed",failure);
    }
    static int Main(string[] args) {
        try {
            Thread.CurrentThread.CurrentCulture=CultureInfo.InvariantCulture;Thread.CurrentThread.CurrentUICulture=CultureInfo.InvariantCulture;
            typeof(Simulator).GetProperty("Catalog",BindingFlags.Public|BindingFlags.Static)
                .SetValue(null,new GettextResourceManager("Orts.Simulation"),null);
            if(args[0]=="steam-drive"){SteamDrive(args[1],args[2],args[3],JObject.Parse(File.ReadAllText(args[4])));return 0;}
            var input=JObject.Parse(File.ReadAllText(args[0]));
            Json(args[1],new {reference_version="1.6.1",reference_commit="d16e670da333d26d2edfc97d5631a19dadf49ce5",
                capture_kind="original_dll_calls",properties=Properties(input),brakes=Brakes(input),brake_networks=BrakeNetworks(input),
                methods=new[]{"SteamTable factory methods / Interpolator.Item","AirSinglePipe.Parse/Initialize/Update/PropagateBrakePressure"}});
            return 0;
        } catch(Exception e) { Console.Error.WriteLine(e);return 1; }
    }
}
