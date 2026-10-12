// Calls the unmodified OR 1.6.1 load-physics method. Fuel-only cases: no sand
// or heating water, constant empty/full Davis and brake coefficients.
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.Serialization;
using Newtonsoft.Json;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.RollingStocks;
class ConsumableMassReference {
 static object Empty(Type t){return FormatterServices.GetUninitializedObject(t);}
 static FieldInfo Field(Type t,string name){for(;t!=null;t=t.BaseType){var f=t.GetField(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);if(f!=null)return f;}throw new MissingFieldException(name);}
 static void Set(object obj,string name,object value){Field(obj.GetType(),name).SetValue(obj,value);}
 static void Main(string[] args){
  var loco=(MSTSDieselLocomotive)Empty(typeof(MSTSDieselLocomotive));
  loco.Train=(Train)Empty(typeof(Train));loco.Train.Cars=new List<TrainCar>{loco};
  var freight=Empty(Field(loco.GetType(),"FreightAnimations").FieldType);Set(freight,"ContinuousFreightAnimationsPresent",true);Set(loco,"FreightAnimations",freight);
  var controller=Activator.CreateInstance(Field(loco.GetType(),"FuelController").FieldType,new object[]{0f,1f,.0025f});Set(loco,"FuelController",controller);
  loco.MaxDieselLevelL=5000f;loco.DieselWeightKgpL=.8508f;
  Set(loco,"LoadEmptyMassKg",75746f);Set(loco,"LoadFullMassKg",80000f);Set(loco,"InitialMassKG",80000f);Set(loco,"InitialDrvWheelWeightKg",36000f);
  foreach(var n in new[]{"LoadEmptyMaxBrakeForceN","LoadFullMaxBrakeForceN"})Set(loco,n,100000f);
  foreach(var n in new[]{"LoadEmptyORTSDavis_A","LoadFullORTSDavis_A"})Set(loco,n,400f);
  var method=typeof(MSTSWagon).GetMethod("UpdateLocomotiveLoadPhysics",BindingFlags.NonPublic|BindingFlags.Instance);
  var cases=new List<object>();foreach(float liters in new[]{5000f,2500f,1f,0f,5000f}){
   loco.DieselLevelL=liters;method.Invoke(loco,null);cases.Add(new{fuel_l=liters,mass_kg=loco.MassKG,driven_mass_kg=loco.DrvWheelWeightKg});
  }
  File.WriteAllText(args[0],JsonConvert.SerializeObject(new{version="1.6.1",scope="Original UpdateLocomotiveLoadPhysics, continuous-freight flag, fuel only, then refill",density_kg_per_l=loco.DieselWeightKgpL,full_mass_kg=80000f,capacity_l=5000f,points=cases},Formatting.Indented));
 }
}
