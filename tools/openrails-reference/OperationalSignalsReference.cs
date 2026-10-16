// Invoke unchanged Open Rails 1.6.1 methods; no renderer, audio device or player session.
using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.Serialization;
using Newtonsoft.Json;
using Orts.Formats.Msts;
using Orts.Simulation.Physics;
using Orts.Simulation.Signalling;

class OperationalSignalsReference {
    static T Empty<T>() { return (T)FormatterServices.GetUninitializedObject(typeof(T)); }
    static void Set(object obj,string name,object value) {
        for(Type t=obj.GetType();t!=null;t=t.BaseType) {
            var p=t.GetProperty(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(p!=null) {p.SetValue(obj,value,null);return;}
            var f=t.GetField(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(f!=null) {f.SetValue(obj,value);return;}
        }
        throw new MissingMemberException(obj.GetType().Name,name);
    }
    static SignalObject Signal() {var s=Empty<SignalObject>();s.SignalHeads=new List<SignalHead>();Set(s,"passedSections",new List<int>());return s;}
    static void Main(string[] args) {
        if(args.Length!=2)throw new ArgumentException("OperationalSignalsReference INPUT.json OUTPUT.json");
        var input=Newtonsoft.Json.Linq.JObject.Parse(File.ReadAllText(args[0]));
        var blocks=new List<object>();
        foreach(var name in input["internal_block_states"]) {
            var state=(SignalObject.InternalBlockstate)Enum.Parse(typeof(SignalObject.InternalBlockstate),(string)name);
            var signal=Signal();Set(signal,"internalBlockState",state);
            blocks.Add(new{internal_state=name.ToString(),result=(int)signal.block_state()});
        }
        var links=new List<object>();
        foreach(var scenario in input["links"]) {
            int required=(int)scenario["required_vector"], path=(int)scenario["route_vector"];
            bool enabled=(bool)scenario["enabled"];
            var db=Empty<TrackDB>();db.TrackNodes=new TrackNode[4];
            for(int i=0;i<4;i++) {db.TrackNodes[i]=Empty<TrackNode>();db.TrackNodes[i].TCCrossReference=new TrackCircuitXRefList{new TrackCircuitSectionXref(i,100,new float[2])};}
            var signals=Empty<Signals>();Set(signals,"trackDB",db);
            var circuits=new List<TrackCircuitSection>();
            for(int i=0;i<4;i++) {
                var c=Empty<TrackCircuitSection>();c.OriginalIndex=i;c.Index=i;c.CircuitType=TrackCircuitSection.TrackCircuitType.Normal;
                c.ActivePins=new TrPin[2,2];c.Pins=new TrPin[2,2];
                for(int a=0;a<2;a++)for(int b=0;b<2;b++) {c.ActivePins[a,b]=new TrPin{Link=-1,Direction=-1};c.Pins[a,b]=new TrPin{Link=-1,Direction=-1};}
                circuits.Add(c);
            }
            Set(signals,"TrackCircuitList",circuits);
            var train=Empty<Train>();train.ValidRoute=new[]{new Train.TCSubpathRoute(),new Train.TCSubpathRoute()};
            if(path>0) {var element=Empty<Train.TCRouteElement>();Set(element,"TCSectionIndex",path);train.ValidRoute[0].Add(element);}
            var routed=Empty<Train.TrainRouted>();Set(routed,"Train",train);Set(routed,"TrainRouteDirectionIndex",0);
            var signal=Signal();Set(signal,"signalRef",signals);signal.enabledTrain=enabled?routed:null;signal.TCReference=0;
            var head=Empty<SignalHead>();Set(head,"mainSignal",signal);Set(head,"Function",SignalFunction.NORMAL);
            head.TrackJunctionNode=(uint)(required>0?3:0);head.JunctionMainNode=required;signal.SignalHeads.Add(head);
            links.Add(new{required_vector=required,route_vector=path,enabled=enabled,result=head.route_set()});
        }
        var bindings=new List<object>();
        foreach(int path in new[]{0,1}) {
            var nodes=new TrackNode[4];nodes[1]=Empty<TrackNode>();nodes[1].Inpins=1;
            nodes[1].TrPins=new[]{new TrPin{Link=0},new TrPin{Link=2},new TrPin{Link=3}};SignalObject.trackNodes=nodes;
            var signal=Signal();var item=Empty<SignalItem>();item.NoSigDirs=1;
            item.TrSignalDirs=new[]{new SignalItem.StrTrSignalDir{TrackNode=1,LinkLRPath=(uint)path}};
            signal.AddHead(0,0,item);
            bindings.Add(new{junction_path=path,required_vector=signal.SignalHeads[0].JunctionMainNode});
        }
        File.WriteAllText(args[1],JsonConvert.SerializeObject(new{reference="Open Rails 1.6.1",blocks,links,bindings},Formatting.Indented).Replace("\r\n","\n")+"\n");
    }
}
