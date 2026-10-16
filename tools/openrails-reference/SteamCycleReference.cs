// Real unmodified OR1.6.1 DLL calls. No boiler/cylinder/flow/traction equations
// are implemented by this helper. Programmed speed is allowed only in the
// explicitly labelled subsystem case; Simulator.Update determines the drive.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Threading;
using GNU.Gettext;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using ORTS.Settings;
using ORTS.Common;
using ORTS.Scripting.Api;
using Orts.Simulation;
using Orts.Simulation.RollingStocks;
using Orts.Parsers.Msts;

internal static class SteamCycleReference {
    static object Get(object obj,string name) {
        for(Type t=obj.GetType();t!=null;t=t.BaseType) {
            var f=t.GetField(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(f!=null)return f.GetValue(obj);
            var p=t.GetProperty(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(p!=null)return p.GetValue(obj,null);
        }
        throw new MissingMemberException(obj.GetType().FullName,name);
    }
    static void Set(object obj,string name,object value) {
        for(Type t=obj.GetType();t!=null;t=t.BaseType) {
            var f=t.GetField(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(f!=null){f.SetValue(obj,value);return;}
            var p=t.GetProperty(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(p!=null){p.SetValue(obj,value,null);return;}
        }
        throw new MissingMemberException(obj.GetType().FullName,name);
    }
    static float F(object o,string n){return Convert.ToSingle(Get(o,n),CultureInfo.InvariantCulture);}
    static JToken Value(object o) {
        if(o is float && (Single.IsNaN((float)o)||Single.IsInfinity((float)o)))return new JValue(((float)o).ToString(CultureInfo.InvariantCulture));
        if(o is double && (Double.IsNaN((double)o)||Double.IsInfinity((double)o)))return new JValue(((double)o).ToString(CultureInfo.InvariantCulture));
        return o==null?JValue.CreateNull():JToken.FromObject(o);
    }
    static JObject Fields(object obj,IEnumerable<string> names) {
        var r=new JObject();foreach(var name in names)r[name]=Value(Get(obj,name));return r;
    }
    static JObject Primitives(object obj) {
        var r=new JObject();
        for(Type t=obj.GetType();t!=null;t=t.BaseType)foreach(var f in t.GetFields(BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly)) {
            if(f.FieldType.IsPrimitive||f.FieldType.IsEnum||f.FieldType==typeof(decimal))
                r[t.Name+"."+f.Name]=Value(f.GetValue(obj));
        }
        return r;
    }
    static void Json(string p,object obj){File.WriteAllText(p,JsonConvert.SerializeObject(obj,Formatting.Indented).Replace("\r\n","\n")+"\n");}
    static string[] StateFields={"BoilerPressurePSI","BoilerMassLB","BoilerHeatBTU","WaterFraction",
        "BoilerHeatSmoothedBTU","PreviousBoilerHeatSmoothedBTU","BoilerSteamHeatBTUpLB","BoilerWaterHeatBTUpLB",
        "BoilerSteamDensityLBpFT3","BoilerWaterDensityLBpFT3","BoilerWaterTempK","WaterTempNewK","BkW_Diff",
        "FlueTempK","BoilerKW","EvaporationLBpS","BoilerHeatInBTUpS","BoilerHeatOutBTUpS","PreviousBoilerHeatOutBTUpS",
        "TotalSteamUsageLBpS","PreviousTotalSteamUsageLBpS","CylinderSteamUsageLBpS","CylCockSteamUsageLBpS",
        "BlowerSteamUsageLBpS","BlowerBurnEffect","DamperBurnEffect","RadiationSteamLossLBpS",
        "FireMassKG","FireRatio","BurnRateRawKGpS","FuelBurnRateSmoothedKGpS","FuelFeedRateKGpS","FuelFeedRateSmoothedKGpS",
        "FireHeatLossPercent","TempFireHeatLossPercent","BoilerWaterInputLB","WaterConsumptionLbpS",
        "CurrentLocoTenderWaterVolumeUKG","CoalIsExhausted","WaterIsExhausted","FireIsExhausted",
        "FusiblePlugIsBlown","BoilerIsPriming","BlowerIsOn","SafetyIsOn","safety2IsOn","safety3IsOn","safety4IsOn",
        "SafetyValveUsageLBpS","SafetyValveUsage1LBpS","SafetyValveUsage2LBpS","SafetyValveUsage3LBpS","SafetyValveUsage4LBpS",
        "CurrentSuperheatTempF","SuperheatVolumeRatio","SuperheaterSteamUsageFactor","IsSuperSet",
        "Injector1Fraction","Injector2Fraction","Injector1WaterDelTempF","Injector2WaterDelTempF","InjectorFlowRateLBpS",
        "MaxInjectorFlowRateLBpS","InjectorSize","InjectorLockedOut","InjectorLockOutTimeS","WaterGlassLevelIN",
        "SmallEjectorFeedFraction","LargeEjectorFeedFraction","SmallSteamEjectorIsOn","LargeSteamEjectorIsOn",
        "EjectorTotalSteamConsumptionLbpS","SmallEjectorBrakePipeChargingRatePSIorInHgpS","LargeEjectorBrakePipeChargingRatePSIorInHgpS",
        "VacuumPumpChargingRateInHgpS","VacuumPumpOperating","GeneratorSteamUsageLBpS","GeneratorSteamEffects",
        "SandingSteamUsageLBpS","StokerSteamUsageLBpS","CumulativeCylinderSteamConsumptionLbs","CumulativeWaterConsumptionLbs",
        "CummulativeTotalSteamConsumptionLbs","MaxForceN","MotiveForceN","MassKG","CarOutsideTempC","InitialCarOutsideTempC","CarHeightAboveSeaLevelM"};
    static string[] HardwareFields={"BoilerVolumeFT3","EvaporationAreaM2","GrateAreaM2","IdealFireMassKG","MaxFireMassKG",
        "MaxFuelBurnGrateKGpS","TheoreticalMaxSteamOutputLBpS","MaxBoilerOutputLBpH","MaxFiringRateLbpH","MaxFiringRateKGpS",
        "GrateLimitLBpFt2","MaxBoilerHeatBTU","MaxBoilerSafetyPressHeatBTU","HasSuperheater","IsSaturated",
        "MaxSuperheatRefTempF","SuperheatTempRatio","CylinderClearancePC","SuperheatCutoffPressureFactor","CylinderPortOpeningFactor",
        "CylinderEfficiencyRate","MaxTractiveEffortLbf","MaxIndicatedHorsePowerHP","BoilerSurfaceAreaFt2","FractionBoilerAreaInsulated",
        "KcInsulation","BlowerSteamUsageFactor","EjectorSmallSteamConsumptionLbpS","EjectorLargeSteamConsumptionLbpS","WaterLBpUKG",
        "MaxTenderFuelMassKG","MaxLocoTenderWaterMassKG","IsTenderRequired","HasTenderCoupled","HotStart","FiringIsManual",
        "MSTSNumCylinders","MSTSCylinderDiameterM","MSTSCylinderStrokeM","DriverWheelRadiusM","CylinderPistonShaftDiaIn",
        "BrakePipeChargingRatePSIorInHgpS","BrakePipeTimeFactorS","BrakePipeDischargeTimeFactor","BrakeServiceTimeFactorPSIpS",
        "BrakeEmergencyTimeFactorPSIpS","TrainBrakePipeLeakPSIorInHgpS","MaxVaccuumMaxPressurePSI","SmallEjectorControllerFitted",
        "LargeEjectorControllerFitted","VacuumPumpFitted","VacuumBrakeEQFitted","SteamEngineBrakeFitted","EngineBrakeFitted"};
    static string[] EngineFields={"DriveWheelRevRpS","MeanEffectivePressurePSI","CylinderSteamUsageLBpS","CylCockSteamUsageLBpS",
        "CylinderCocksPressureAtmPSI","Pressure_a_AtmPSI","Pressure_b_AtmPSI","Pressure_c_AtmPSI","Pressure_d_AtmPSI",
        "Pressure_e_AtmPSI","Pressure_f_AtmPSI","IndicatedHorsePowerHP","RealTractiveForceN","PistonSpeedFtpMin"};
    static string[] BodyFields={"FrictionForceN","BrakeShoeForceN","BrakeRetardForceN","BrakeForceN",
        "CurveForceN","CurveForceNFiltered","WindForceN","TunnelForceN","GravityForceN","TotalForceN",
        "MotiveForceN","TractiveForceN","SpeedMpS","MassKG","CarLengthM","CarBogieCentreLengthM",
        "CurrentCurveRadiusM","SuperElevationM","SuperElevationAngleRad","TrackGaugeM","RigidWheelBaseM",
        "CurrentElevationPercent","DavisAN","DavisBNSpM","DavisCNSSpMM","IsDavisFriction",
        "FrictionC1","FrictionE1","FrictionV2","FrictionC2","FrictionE2","Friction0N",
        "MaxBrakeForceN","MaxHandbrakeForceN","BrakeSkid","IsStandStill"};
    static object Filter(object l,string name){var o=Get(l,name);return new{value=Value(Get(o,"SmoothedValue")),period_s=F(o,"SmoothPeriodS")};}
    static object InitialWorldPosition(object l){
        // Static boundary after Simulator.Start/CalculatePositionOfCars. This
        // is not a later trajectory state or a measured temperature input.
        var w=Get(l,"WorldPosition");var v=Get(w,"Location");
        return new{tile_x=Get(w,"TileX"),tile_z=Get(w,"TileZ"),
            location_x=F(v,"X"),location_y=F(v,"Y"),location_z=F(v,"Z"),
            convention="MSTS WorldPosition.Location; latitude from original WorldLatLon.ConvertWTC"};
    }
    static object Body(object l){
        var w=Get(l,"WorldPosition");var matrix=Get(w,"XNAMatrix");
        return new{car=Fields(l,BodyFields),world_position=InitialWorldPosition(l),
            xna_matrix=Fields(matrix,new[]{"M11","M12","M13","M14","M21","M22","M23","M24",
                "M31","M32","M33","M34","M41","M42","M43","M44"}),
            train_friction=Fields(Get(l,"Train"),new[]{"WagonCoefficientFriction","LocomotiveCoefficientFriction"}),
            scope="Additive read-only body telemetry; no changed controls, numerical state, timestep or native limits"};
    }
    static object Tables(){
        var type=typeof(Simulator).Assembly.GetType("Orts.Common.SteamTable",true);
        var points=new List<object>();
        foreach(var m in type.GetMethods(BindingFlags.Public|BindingFlags.Static).OrderBy(m=>m.Name)){
            if(m.GetParameters().Length!=0||m.ReturnType!=typeof(Interpolator))continue;
            var t=(Interpolator)m.Invoke(null,null);
            foreach(float fraction in new[]{0f,.13f,.5f,.9f,1f}){
                float x=t.MinX()+(t.MaxX()-t.MinX())*fraction;
                points.Add(new{factory=m.Name,x=x,value=t[x],domain_min=t.MinX(),domain_max=t.MaxX()});
            }
        }
        var grids=new List<object>();
        foreach(string factory in new[]{"CutoffInitialPressureUpper","CutoffInitialPressureLower"}){
            var t=(Interpolator2D)type.GetMethod(factory,BindingFlags.Public|BindingFlags.Static).Invoke(null,null);
            foreach(float rpm in new[]{0f,12f,50f,110f,350f})foreach(float cutoff in new[]{.1f,.3f,.45f,.65f,.75f,.9f})
                grids.Add(new{factory=factory,rpm=rpm,cutoff=cutoff,value=t.Get(rpm,cutoff)});
        }
        return new{method="Original SteamTable factory/Interpolator.Item/Interpolator2D.Get",points=points,grids=grids};
    }
    static object Sample(MSTSSteamLocomotive l,int tick,JObject command) {
        return new {tick=tick,phase=command==null?"initial":(string)command["phase"],
            requested=command,speed_mps=l.Train.SpeedMpS,distance_m=l.Train.DistanceTravelledM,
            pressure_psi=F(l,"BoilerPressurePSI"),boiler_mass_lb=l.BoilerMassLB,boiler_heat_btu=F(l,"BoilerHeatBTU"),
            water_fraction=F(l,"WaterFraction"),evaporation_lb_s=l.EvaporationLBpS,steam_usage_lb_s=l.PreviousTotalSteamUsageLBpS,
            water_consumption_lb_s=l.WaterConsumptionLbpS,heat_in_btu_s=l.BoilerHeatInBTUpS,heat_out_btu_s=l.PreviousBoilerHeatOutBTUpS,
            motive_force_n=l.MotiveForceN,fire_mass_kg=l.FireMassKG,tender_coal_kg=l.TenderFuelMassKG,
            combined_tender_water_ukg=l.CombinedTenderWaterVolumeUKG,water_lb_per_ukg=l.WaterLBpUKG,
            injector1=l.Injector1IsOn,injector2=l.Injector2IsOn,injector1_fraction=F(l,"Injector1Fraction"),injector2_fraction=F(l,"Injector2Fraction"),
            fire_burn_kg_s=l.FuelBurnRateSmoothedKGpS,regulator=l.ThrottlePercent*.01f,cutoff=l.CutoffController.CurrentValue,
            damper=l.DamperController.CurrentValue,firing=l.FiringRateController.CurrentValue,
            blower=l.BlowerController.CurrentValue,injector_controllers=new[]{l.Injector1Controller.CurrentValue,l.Injector2Controller.CurrentValue},
            locomotive=Fields(l,StateFields),engine=Fields(l.SteamEngines[0],EngineFields),brake=Primitives(l.BrakeSystem),
            body=Body(l),
            boiler_filter=Filter(l,"BoilerHeatSmoothBTU"),burn_filter=Filter(l,"BurnRateCoalSmoothKGpS"),
            fuel_controller=l.FuelController.CurrentValue,water_controller=l.WaterController.CurrentValue,
            train_brake_handle=l.TrainBrakeController.CurrentValue,train_brake_kind=l.TrainBrakeController.TrainBrakeControllerState.ToString()};
    }
    static void Apply(MSTSSteamLocomotive l,JObject c,float dt) {
        l.SetThrottlePercentWithSound((float)c["throttle"]*100);l.SetTrainBrakePercent((float)c["brake"]*100);
        l.CutoffController.SetValue((float)c["cutoff"]);l.CutoffController.Update(dt);
        l.DamperController.SetValue((float)c["damper"]);l.DamperController.Update(dt);
        l.FiringRateController.SetValue((float)c["firing"]);l.FiringRateController.Update(dt);
        // Identical to historical05: controller is explicitly ordered0/1,
        // not inferred from the boolean or from the resulting injector flow.
        l.Injector1Controller.SetValue((bool)c["injector1"]?1:0);l.Injector1Controller.Update(dt);
        l.Injector2Controller.SetValue((bool)c["injector2"]?1:0);l.Injector2Controller.Update(dt);
        l.Injector1IsOn=(bool)c["injector1"];l.Injector2IsOn=(bool)c["injector2"];
    }
    static void Capture(string path,string consist,string output,JObject config) {
        Thread.CurrentThread.Name="Loader Process";
        var hot=(bool)config["hot_start"];var paused=(bool)config["paused"];
        var settings=new UserSettings(new[]{"Language=en","SimpleControlPhysics=true","UseAdvancedAdhesion=false",
            "CorrectQuestionableBrakingParams=false","AdhesionFactor=100","AdhesionFactorChange=0",
            "AdhesionProportionalToWeather=false","CurveSpeedDependent=false","TunnelResistanceDependent=false",
            "WindResistanceDependent=false","Alerter=false","HotStart="+hot.ToString().ToLowerInvariant(),
            "ActRandomizationLevel=0","ActWeatherRandomizationLevel=0","NoForcedRedAtStationStops=true"});
        var sim=new Simulator(settings,path,false,true);sim.Confirmer=new Confirmer(sim,1);
        sim.SetExplore(path,consist,"9:29","1","0");sim.Start(new ORTS.Common.CancellationTokenSource(delegate{}).Token);
        sim.SetCommandReceivers();sim.PreUpdate=false;sim.Paused=paused;
        Directory.CreateDirectory(output);Exception failure=null;
        Json(Path.Combine(output,"tables.json"),Tables());
        var worker=new Thread(delegate(){try{
            var l=sim.PlayerLocomotive as MSTSSteamLocomotive;
            if(l==null||l.SteamEngines.Count!=1)throw new InvalidOperationException("Expected one original simple steam engine");
            Set(l,"FiringIsManual",true);l.SetDirection(Direction.Forward);
            Json(Path.Combine(output,"initial.json"),new{hardware=Fields(l,HardwareFields),locomotive=Primitives(l),
                world_position=InitialWorldPosition(l),
                engine=Primitives(l.SteamEngines[0]),brake=Primitives(l.BrakeSystem),train=Primitives(l.Train),
                settings=new{paused=sim.Paused,hot_start=hot,simple_control_physics=sim.Settings.SimpleControlPhysics,
                    advanced_adhesion=sim.UseAdvancedAdhesion},sample=Sample(l,0,null)});
            var rows=new List<object>();rows.Add(Sample(l,0,null));
            var commands=(JArray)config["commands"];int command=0;float dt=(float)config["dt_s"];
            bool prescribed=(string)config["kind"]=="prescribed_speed_subsystem";
            for(int tick=0;tick<(int)config["ticks"];tick++) {
                while(command+1<commands.Count&&(int)commands[command+1]["tick"]<=tick)command++;
                var c=(JObject)commands[command];Apply(l,c,dt);
                if(prescribed){
                    float speed=(float)c["speed_mps"];l.Train.SpeedMpS=speed;
                    foreach(var car in l.Train.Cars)car.SpeedMpS=speed;
                    // Boundary is the authored programme, never an observed
                    // post-start state from a different original trajectory.
                    l.BrakeSystem.BrakeLine1PressurePSI=(float)c["pipe_absolute_psi"];
                    l.Update(dt);
                }else{sim.Update(dt);}
                rows.Add(Sample(l,tick+1,c));
            }
            Json(Path.Combine(output,"reference.json"),new{reference_version="1.6.1",
                reference_commit="d16e670da333d26d2edfc97d5631a19dadf49ce5",input=config,
                method=prescribed?"MSTSSteamLocomotive.Update":"Simulator.Update",rows=rows,
                scope=prescribed?"Prescribed subsystem speed and external pipe boundary; not a drive or a train physics oracle.":
                    "Original closed-loop drive; no prescribed speeds, forces, pressures, heat or steam flows."});
        }catch(Exception e){failure=e;}});worker.Name="Updater Process";worker.Start();worker.Join();
        if(failure!=null)throw new InvalidOperationException("SteamCycleReference original call failed",failure);
    }
    static int Main(string[] args){try{
        Thread.CurrentThread.CurrentCulture=CultureInfo.InvariantCulture;Thread.CurrentThread.CurrentUICulture=CultureInfo.InvariantCulture;
        typeof(Simulator).GetProperty("Catalog",BindingFlags.Public|BindingFlags.Static).SetValue(null,new GettextResourceManager("Orts.Simulation"),null);
        Capture(args[0],args[1],args[2],JObject.Parse(File.ReadAllText(args[3])));return 0;
    }catch(Exception e){Console.Error.WriteLine(e);return 1;}}
}
