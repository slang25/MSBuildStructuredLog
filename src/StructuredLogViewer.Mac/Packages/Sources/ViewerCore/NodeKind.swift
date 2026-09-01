import Foundation

/// The ~35 node kinds the engine emits (CLR type names), matching the
/// template set in the Avalonia viewer's App.xaml. UI styling (SF Symbol +
/// color) is keyed off this in ViewerUI's NodeStyling.
public enum NodeKind: String, Sendable, CaseIterable {
    case build = "Build"
    case project = "Project"
    case projectEvaluation = "ProjectEvaluation"
    case target = "Target"
    case task = "Task"
    case addItem = "AddItem"
    case removeItem = "RemoveItem"
    case item = "Item"
    case metadata = "Metadata"
    case property = "Property"
    case parameter = "Parameter"
    case folder = "Folder"
    case message = "Message"
    case timedMessage = "TimedMessage"
    case criticalBuildMessage = "CriticalBuildMessage"
    case error = "Error"
    case warning = "Warning"
    case note = "Note"
    case importNode = "Import"
    case noImport = "NoImport"
    case entryTarget = "EntryTarget"
    case package = "Package"
    case fileCopy = "FileCopy"
    case sourceFile = "SourceFile"
    case sourceFileLine = "SourceFileLine"
    case evaluationProfileEntry = "EvaluationProfileEntry"
    case msBuildServerNode = "MSBuildServerNode"
    case timedNode = "TimedNode"
    case taskParameterItem = "TaskParameterItem"
    case taskParameterProperty = "TaskParameterProperty"
    case proxy = "ProxyNode"
    case unknown = ""

    public init(kindString: String) {
        self = NodeKind(rawValue: kindString) ?? .unknown
    }
}

extension NodeSummary {
    public var nodeKind: NodeKind { NodeKind(kindString: kind) }
}
