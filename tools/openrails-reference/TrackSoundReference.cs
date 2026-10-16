// Original WORLD/STF/Traveller and WorldSounds.GetTType from OR 1.6.1.
// A minimal read-only viewer context supplies the track DB; no renderer/audio starts.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Serialization;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using Orts.Formats.Msts;
using Orts.Simulation;
using Orts.Simulation.Physics;
using Orts.Viewer3D;

class TrackSoundReference {
    static T Empty<T>() {return (T)FormatterServices.GetUninitializedObject(typeof(T));}
    static void Set(object o,string name,object value) {
        for(Type t=o.GetType();t!=null;t=t.BaseType) {
            var p=t.GetProperty(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(p!=null){p.SetValue(o,value,null);return;}
            var f=t.GetField(name,BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly);
            if(f!=null){f.SetValue(o,value);return;}
        }
        throw new MissingMemberException(o.GetType().Name,name);
    }
    static string Edge(Traveller t) {return "e"+t.TrackNodeIndex+(t.Direction==Traveller.TravellerDirection.Forward?"":"_r");}
    static string Reverse(string edge) {return edge.EndsWith("_r")?edge.Substring(0,edge.Length-2):edge+"_r";}
    static List<string> Walk(Traveller original,bool reverse) {
        var t=new Traveller(original,reverse?Traveller.TravellerDirection.Backward:Traveller.TravellerDirection.Forward);
        var path=new List<string>();var seen=new HashSet<string>();float distance=0;
        for(int i=0;i<4096;i++) {
            if(t.IsTrack) {var edge=Edge(t);if(!seen.Add(edge))break;path.Add(edge);distance+=t.TrackNodeLength;if(distance>16384)break;}
            if(!t.NextTrackNode())break;
        }
        return path;
    }
    static void Run(string[] args) {
        if(args.Length!=4)throw new ArgumentException("TrackSoundReference INPUT.json ROUTE GLOBAL/tsection.dat OUTPUT.json");
        var input=JObject.Parse(File.ReadAllText(args[0]));string route=args[1];
        var db=new TrackDatabaseFile(Directory.GetFiles(route).First(p=>p.EndsWith(".tdb",StringComparison.OrdinalIgnoreCase)));
        var sections=new TrackSectionsFile(args[2]);
        var routeSections=Directory.GetFiles(route).FirstOrDefault(p=>Path.GetFileName(p).Equals("tsection.dat",StringComparison.OrdinalIgnoreCase));
        if(routeSections!=null)sections.AddRouteTSectionDatFile(routeSections);
        var sim=Empty<Simulator>();sim.TDB=db;var viewer=Empty<Viewer>();Set(viewer,"Simulator",sim);
        var sounds=new WorldSounds(viewer);var all=new Dictionary<string,List<WorldSoundRegion>>();
        foreach(string ws in Directory.GetFiles(Path.Combine(route,"WORLD"),"*.ws").OrderBy(p=>p,StringComparer.Ordinal)) {
            var file=new WorldSoundFile(ws,db.TrackDB.TrItemTable);if(file.TR_WorldSoundFile!=null)all.Add(Path.GetFileName(ws),file.TR_WorldSoundFile.SoundRegions);
        }
        Set(sounds,"SoundRegions",all);
        var selected=all.Values.SelectMany(v=>v).Where(r=>r.TrackNodes.Count>0).GroupBy(r=>r.SoundRegionTrackType).Select(g=>g.First()).Take((int)input["maximum_types"]).ToArray();
        var rows=new List<object>();
        foreach(var region in selected) {
            int id=region.TrackNodes.First();var item=db.TrackDB.TrItemTable[id];
            foreach(float offset in input["offsets_m"].Select(v=>(float)v)) {
                var probe=new Traveller(sections,db.TrackDB.TrackNodes,item.TileX,item.TileZ,item.X,item.Z,Traveller.TravellerDirection.Forward);probe.Move(offset);
                if(!probe.IsTrack)continue;
                var before=Walk(probe,true);before.Reverse();before=before.Select(Reverse).ToList();
                if(before.Count>0)before.RemoveAt(before.Count-1);
                before.AddRange(Walk(probe,false));
                foreach(bool reverse in new[]{false,true}) {
                    var train=Empty<Train>();train.FrontTDBTraveller=new Traveller(probe);train.RearTDBTraveller=new Traveller(probe);train.SpeedMpS=reverse?-10:10;
                    float previous,next;int type=sounds.GetTType(train,out previous,out next);
                    rows.Add(new{anchor_item=id,anchor_type=region.SoundRegionTrackType,offset_m=offset,reverse=reverse,edge=Edge(probe),position_m=probe.TrackNodeOffset,path=before,result=type==int.MaxValue?(int?)null:type,previous_distance_m=previous,next_distance_m=next});
                }
            }
        }
        File.WriteAllText(args[3],JsonConvert.SerializeObject(new{reference="Open Rails 1.6.1",world_files=all.Count,types=selected.Select(r=>r.SoundRegionTrackType),rows},Formatting.Indented).Replace("\r\n","\n")+"\n");
    }
    static void Main(string[] args) {try {Run(args);}catch(Exception e){Console.Error.WriteLine(e);Environment.Exit(1);}}
}
