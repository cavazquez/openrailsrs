// Read native model composition using the original Orts.Formats.Msts.dll.
using System;
using System.IO;
using Orts.Formats.Msts;
using Newtonsoft.Json;

class InspectShapes
{
    static void Main(string[] args)
    {
        if (args.Length < 2)
            throw new ArgumentException("InspectShapes <output.json> <shape.s> ...");
        var shapes = new System.Collections.Generic.List<object>();
        for (int i = 1; i < args.Length; i++)
            shapes.Add(new { file = Path.GetFileName(args[i]), shape = new ShapeFile(args[i], true).shape });
        File.WriteAllText(args[0], JsonConvert.SerializeObject(shapes, Formatting.Indented));
    }
}
