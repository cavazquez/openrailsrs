// Capture the unmodified OR 1.6.1 STF/Path readers, never the Rust parser.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Runtime.Serialization;
using Newtonsoft.Json;
using Orts.Formats.Msts;
using Orts.Parsers.Msts;
class ParserReference {
    static void Main(string[] args) {
        if(args.Length!=1) throw new ArgumentException("ParserReference OUTPUT.json");
        var root=Path.Combine(Path.GetDirectoryName(args[0]),"parser-fixtures");
        Directory.CreateDirectory(root); Directory.CreateDirectory(Path.Combine(root,"INCLUDE"));
        // STF's filename constructor skips the first line; original include files
        // start with a blank/header line too.
        File.WriteAllText(Path.Combine(root,"INCLUDE","mass.inc"),"\nMass ( 42t )",Encoding.Unicode);
        var wagon=Path.Combine(root,"car.wag");
        File.WriteAllText(wagon,"SIMISA@@@@@@@@@@JINX0D0t______\nWagon ( car Include ( \"/INCLUDE/mass.inc\" ) Size ( 3m 4m 20m ) )",Encoding.Unicode);
        var include=new List<string>();
        using(var stf=new STFReader(wagon,false)) {
            while(!stf.Eof) {var value=stf.ReadItem();if(value.Length>0) include.Add(value);}
        }
        var eof=Path.Combine(root,"legacy.act");
        File.WriteAllText(eof,"SIMISA@@@@@@@@@@JINX0a0t______\nTr_Activity ( Tr_Activity_File ( Events ( EventCategoryTime ( Outcomes ( DisplayMessage ( \"Legacy tutorial text at EOF",Encoding.Unicode);
        var trailing=new List<string>();
        using(var stf=new STFReader(eof,false)) {
            while(!stf.Eof) {var value=stf.ReadString();if(value.Length>0) trailing.Add(value);}
        }
        var late=Path.Combine(root,"late-quote.act");
        File.WriteAllText(late,"SIMISA@@@@@@@@@@JINX0a0t______\nTr_Activity ( Name ( \"Legacy\" ) Tr_Activity_File ( Events ( EventCategoryLocation ( Outcomes ( DisplayMessage ( \"Legacy end text.\nReturn to depot.\n ) ) Name ( \"End\" ) Location (0 0 0 0 10) ) ) ) ActivityObjects ( ActivityObject ( ObjectType ( WagonsList ) Name ( \"Static\" ) ) ) )",Encoding.Unicode);
        var late_quote=new List<string>();
        using(var stf=new STFReader(late,false)) {
            while(!stf.Eof) {var value=stf.ReadString();if(value.Length>0) late_quote.Add(value);}
        }
        var pat=Path.Combine(root,"flags.pat");
        File.WriteAllText(pat,"SIMISA@@@@@@@@@@JINX0P0t______\nTrackPDPs ( TrackPDP (0 0 0 0 0 1 1) TrackPDP (0 0 100 0 0 1 1) TrackPDP (0 0 200 0 0 1 1) ) TrackPath (TrPathName ( Flags ) TrPathNodes (3 TrPathNode (00000000 1 4294967295 0) TrPathNode (000A0001 2 4294967295 1) TrPathNode (001E0002 4294967295 4294967295 2)))",Encoding.Unicode);
        var path=new PathFile(pat);
        var flags=path.TrPathNodes.Select(node=>node.pathFlags).ToArray();
        var point_flags=new List<object>();
        foreach(var value in new [] {-1,0,1,2,3,9}) {
            var point=(TrackPDP)FormatterServices.GetUninitializedObject(typeof(TrackPDP));
            point.junctionFlag=value;point.invalidFlag=value;
            point_flags.Add(new {junction_flag=value,invalid_flag=value,is_junction=point.IsJunction,is_invalid=point.IsInvalid});
        }
        var train=(Orts.Simulation.Physics.Train)FormatterServices.GetUninitializedObject(typeof(Orts.Simulation.Physics.Train));
        var absolute_waits=new List<object>();
        foreach(var sample in new [] {new [] {31701,61170},new [] {31701,61400},new [] {30001,86380},new [] {30001,120},new [] {30,61170}}) {
            var delay=sample[0];train.TestAbsDelay(ref delay,sample[1]);
            absolute_waits.Add(new {encoded=sample[0],clock_s=sample[1],delay_s=delay});
        }
        File.WriteAllText(args[0],JsonConvert.SerializeObject(new {include=include,trailing=trailing,late_quote=late_quote,flags=flags,absolute_waits=absolute_waits,point_flags=point_flags},Formatting.Indented),new UTF8Encoding(false));
    }
}
