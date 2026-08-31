using System.Text.Json;
using System.Text.Json.Serialization;

namespace StructuredLogViewer.NativeBridge;

[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull)]
[JsonSerializable(typeof(NodeSummaryDto))]
[JsonSerializable(typeof(NodeDetailsDto))]
[JsonSerializable(typeof(ChildrenPageDto))]
[JsonSerializable(typeof(AncestorsDto))]
[JsonSerializable(typeof(SearchResponseDto))]
[JsonSerializable(typeof(SourceLocationDto))]
[JsonSerializable(typeof(FileListDto))]
[JsonSerializable(typeof(FileSearchResponseDto))]
[JsonSerializable(typeof(BuildInfoDto))]
[JsonSerializable(typeof(StatsDto))]
[JsonSerializable(typeof(TimelineDto))]
[JsonSerializable(typeof(ProjectGraphDto))]
[JsonSerializable(typeof(ErrorDto))]
public partial class BridgeJsonContext : JsonSerializerContext
{
    public static string SerializeError(string code, string message) =>
        JsonSerializer.Serialize(new ErrorDto { Code = code, Message = message }, Default.ErrorDto);
}
