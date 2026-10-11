// Execute the original locomotive GetDataOf methods with explicit forces.
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.Serialization;
using Newtonsoft.Json;
using Orts.Formats.Msts;
using Orts.Simulation.RollingStocks;
using Orts.Simulation.RollingStocks.SubSystems.PowerTransmissions;
class CabReference {
    static T Empty<T>() {return (T)FormatterServices.GetUninitializedObject(typeof(T));}
    static void Main(string[] args) {
        var rows=new List<object>();
        foreach(var kind in new[]{CABViewControlTypes.AMMETER,CABViewControlTypes.AMMETER_ABS,CABViewControlTypes.LOAD_METER})
        foreach(int axles in new[]{0,1}) foreach(float current in new[]{0f,600f})
        foreach(float force in new[]{0f,20000f,80000f,120000f}) {
            var loco=Empty<MSTSLocomotive>(); loco.LocomotiveAxles=new Axles(loco);
            if(axles>0) loco.LocomotiveAxles.Add(new Axle());
            loco.MaxForceN=100000; loco.MaxCurrentA=current;
            loco.TractionForceN=force; loco.TractiveForceN=force*.75f;
            var cvc=Empty<CVCDial>(); cvc.ControlType=new CabViewControlType(kind);
            cvc.MinValue=-1000; cvc.MaxValue=1000;
            // GetUninitializedObject skips the normal CVF constructor defaults.
            // Without a unit scale of one, GetDataOf multiplies every result by zero.
            cvc.UnitsScale=1; cvc.UnitsExponent=1;
            float result=loco.GetDataOf(cvc);
            if(force>0 && result<=0) throw new InvalidOperationException("Empty current-meter reference");
            rows.Add(new {kind=kind.ToString(),axles=axles,current_a=current,max_force_n=100000,motor_force_n=force,rail_force_n=force*.75f,scale_min=-1000,scale_max=1000,result=result});
        }
        File.WriteAllText(args[0],JsonConvert.SerializeObject(new {reference="Open Rails 1.6.1",meters=rows,scope="Original CVCDial AMMETER, AMMETER_ABS and LOAD_METER, with/without native axles, explicit and CVF-derived MaxCurrent. Prescribed force inputs, not traction-dynamics parity."},Formatting.Indented).Replace("\r\n","\n")+"\n");
    }
}
