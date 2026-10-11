// Execute unchanged OR 1.6.1 signal methods with explicit, isolated inputs.
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.Serialization;
using Newtonsoft.Json;
using Orts.Formats.Msts;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Simulation.Signalling;

class SignalReference {
    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static void Set(object obj, string name, object value) {
        for (Type t=obj.GetType(); t!=null; t=t.BaseType) {
            var p=t.GetProperty(name, BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly);
            if(p!=null) { p.SetValue(obj,value,null); return; }
            var f=t.GetField(name, BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly);
            if(f!=null) { f.SetValue(obj,value); return; }
        }
        throw new MissingMemberException(obj.GetType().Name,name);
    }
    static SignalObject Signal(int id) {
        var s=Empty<SignalObject>(); s.thisRef=id; s.SignalHeads=new List<SignalHead>();
        s.sigfound=new Dictionary<SignalFunction,int>(); return s;
    }
    static SignalHead Head(SignalFunction f, int aspect) {
        var h=Empty<SignalHead>(); Set(h,"Function",f); h.state=(MstsSignalAspect)aspect; return h;
    }
    static void Main(string[] args) {
        if(args.Length!=1) throw new ArgumentException("SignalReference OUTPUT.json");
        var functions=new[]{SignalFunction.NORMAL,SignalFunction.DISTANCE,SignalFunction.REPEATER,SignalFunction.SHUNTING,SignalFunction.INFO};
        var aggregate=new List<object>();
        for(int f=0;f<functions.Length;f++) for(int a=0;a<8;a++) for(int b=0;b<8;b++) {
            var current=Signal(0); var next=Signal(1); SignalObject.signalObjects=new[]{current,next};
            next.SignalHeads.Add(Head(functions[f],a)); next.SignalHeads.Add(Head(functions[f],b));
            next.SignalHeads.Add(Head(functions[(f+1)%5],0));
            current.sigfound[functions[f]]=1;
            aggregate.Add(new {function=f,a=a,b=b,least=(int)current.next_sig_lr(functions[f]),most=(int)current.next_sig_mr(functions[f])});
        }
        var callon=new List<object>();
        for(int bits=0;bits<64;bits++) {
            bool enabled=(bits&1)!=0, first=(bits&2)!=0, route=(bits&4)!=0;
            bool platform=(bits&8)!=0, manual=(bits&16)!=0, allow=(bits&32)!=0;
            var signal=Signal(0); var train=Empty<Train>();
            train.NextSignalObject=new[]{ first ? signal : Signal(1), (SignalObject)null };
            var routed=Empty<Train.TrainRouted>(); Set(routed,"Train",train); Set(routed,"TrainRouteDirectionIndex",0);
            signal.enabledTrain=enabled ? routed : null;
            signal.CallOnManuallyAllowed=manual;
            var signals=Empty<Signals>();
            var circuit=Empty<TrackCircuitSection>(); circuit.PlatformIndex=new List<int>();
            if(platform) circuit.PlatformIndex.Add(0);
            Set(signals,"TrackCircuitList",new List<TrackCircuitSection>{circuit});
            Set(train,"signalRef",signals); Set(signal,"signalRef",signals);
            signal.signalRoute=route ? new Train.TCSubpathRoute() : null;
            if(route) { var element=Empty<Train.TCRouteElement>(); Set(element,"TCSectionIndex",0); signal.signalRoute.Add(element); }
            callon.Add(new {enabled=enabled,first_signal=first,route_exists=route,leads_into_platform=platform,manually_allowed=manual,allow_non_platform=allow,result=signal.TrainHasCallOn(allow,false,"")});
        }
        var heads=new List<object>();
        for(int i=0;i<32;i++) {
            var signal=Signal(0); var world=Empty<SignalWorldObject>();
            world.HeadsSet=new bool[32]; world.HeadsSet[3]=true; world.HeadsSet[31]=true; signal.WorldObject=world;
            heads.Add(new {mask=2147483656L,index=i,result=signal.HasHead(i)});
        }
        var switches=new List<object>();
        for(int i=-1;i<2;i++) {
            var signal=Signal(0); var signals=Empty<Signals>(); var circuit=Empty<TrackCircuitSection>();
            circuit.JunctionLastRoute=i; Set(signals,"TrackCircuitList",new List<TrackCircuitSection>{circuit});
            Set(signal,"signalRef",signals); signal.nextSwitchIndex=i<0 ? -1 : 0;
            switches.Add(new {route=i,straight=6,diverging=2,result=signal.switchstand(6,2,"")});
        }
        var basic=new List<object>();
        for(int mask=0;mask<256;mask++) {
            var type=Empty<SignalType>(); var aspects=new List<SignalAspect>();
            for(int i=0;i<8;i++) if((mask&(1<<i))!=0) aspects.Add(new SignalAspect((MstsSignalAspect)i,"d"));
            Set(type,"Aspects",aspects);
            basic.Add(new {mask=mask,least=(int)type.GetLeastRestrictiveAspect(),most=(int)type.GetMostRestrictiveAspect()});
        }
        File.WriteAllText(args[0],JsonConvert.SerializeObject(new {
            reference="Open Rails 1.6.1", aggregate=aggregate, call_on=callon, heads=heads, switches=switches, basic=basic,
            scope="NEXT_SIG LR/MR aggregation, ordinary activity call-on guards and platform policy, optional heads and switch stands. Timetable call-on/attach policy is separate."
        },Formatting.Indented).Replace("\r\n","\n")+"\n");
    }
}
