import Foundation

/// The default content shown in the search panes before a query runs: a
/// short syntax primer plus one-click example queries, mirroring the
/// watermark in the WPF and Avalonia viewers.
///
/// Prose is authored as markdown with `[[query]]` placeholders; `render`
/// turns each placeholder into a `mslog-search:` link that the view
/// intercepts and feeds back into the search box.
public enum SearchWatermark {
    /// Node kinds accepted by the `$kind` prefix, in the same order the
    /// desktop viewers list them.
    public static let nodeKinds: [String] = [
        "$project",
        "$projectevaluation",
        "$target",
        "$task",
        "$error",
        "$warning",
        "$message",
        "$property",
        "$item",
        "$additem",
        "$removeitem",
        "$metadata",
        "$csc",
        "$rar",
        "$import",
        "$noimport",
        "$secret",
    ]

    /// Ready-made queries that answer common build questions.
    public static let examples: [String] = [
        "Copying file from ",
        "Resolved file path is ",
        "There was a conflict",
        "Encountered conflict between",
        "Building target completely ",
        "is newer than output ",
        "Property reassignment: $(",
        "out-of-date",
        "$task $time",
        "$message CompilerServer failed",
        "will be compiled because",
        "$secret",
        "$secret not(username)",
    ]

    public static let intro = """
        Type in the search box to search. Press ⌘1 to jump back to this pane. \
        Results (up to %d) will display here.
        """

    public static let matching = """
        Search for multiple words separated by space (space means AND). Enclose multiple words in \
        double quotes to search for the exact phrase. A single word in quotes means exact match \
        (turns off substring search).

        Use syntax like [[$property Prop]] to narrow results down by node kind. Supported kinds:
        """

    public static let clauses = """
         • Use `under(FILTER)` to only include results where any of the nodes in the parent chain matches the FILTER.
         • Use `notunder(...)` as the opposite of `under(...)`.
         • Use `project(...)` to filter by parent project.
         • Use `not(...)` to exclude subqueries.

        Examples:
         • [[$csc under($project Core)]]
         • [[Copying file project(ProjectA.csproj)]]
        """

    public static let modifiers = """
        Use [[$target skipped=false]] to exclude skipped targets (use true to only include skipped).

        Append [[$time]], [[$start]] and/or [[$end]] to show times and/or durations and sort the \
        results by start time or duration descending (for tasks, targets and projects).

        Use `start<"2023-11-23 14:30:54.579"`, `start>`, `end<`, or `end>` to filter events that \
        start or end before or after a given timestamp. Timestamp needs to be in quotes.

        Use `$copy path` where path is a file or directory to find file copy operations involving \
        the file or directory. `$copy substring` will search for copied files containing the substring.

        Use `$nuget project(MyProject.csproj) Package.Name` to search for NuGet packages (by name \
        or version), dependencies (direct and transitive) and files coming from NuGet packages.

        Use `$projectreference project(MyProject.csproj) RefProj` to search for projects referenced \
        by MyProject.csproj directly or indirectly. For a single matching project all referencing \
        projects will be shown as well.
        """

    public static let propertiesAndItems = """
        Look up properties or items for the selected project or a node under a project or \
        evaluation. Properties and items might not be available for some projects.

        Surround the search term in quotes to find an exact match (turns off substring search). \
        Prefix the search term with [[name=]] or [[value=]] to only search property and metadata \
        names or values. Add [[$property ]], [[$item ]] or [[$metadata ]] to limit search to a \
        specific node type.
        """

    /// Replaces every `[[query]]` placeholder with a markdown link that
    /// re-runs `query`, leaving the rest of the text untouched.
    public static func render(_ text: String) -> String {
        var result = ""
        var rest = Substring(text)

        while let open = rest.range(of: "[["), let close = rest.range(of: "]]", range: open.upperBound..<rest.endIndex) {
            result += rest[rest.startIndex..<open.lowerBound]
            result += link(String(rest[open.upperBound..<close.lowerBound]))
            rest = rest[close.upperBound...]
        }

        return result + rest
    }

    /// A markdown link that searches `query`. The visible label defaults to
    /// the query with its trailing space (the "type more here" hint the
    /// desktop viewers use) trimmed off.
    public static func link(_ query: String, label: String? = nil) -> String {
        let text = label ?? query.trimmingCharacters(in: .whitespaces)
        return "[\(text)](\(SearchLink.url(for: query)))"
    }
}

/// URL codec for the watermark's clickable queries. Encoding everything
/// except alphanumerics keeps `$`, spaces, quotes and parentheses out of
/// the markdown link destination.
public enum SearchLink {
    public static let scheme = "mslog-search"

    public static func url(for query: String) -> String {
        let encoded = query.addingPercentEncoding(withAllowedCharacters: .alphanumerics) ?? query
        return "\(scheme):\(encoded)"
    }

    /// The query carried by `url`, or nil if it isn't one of ours.
    public static func query(from url: URL) -> String? {
        guard url.scheme == scheme else { return nil }
        let body = url.absoluteString.dropFirst(scheme.count + 1)
        return body.removingPercentEncoding ?? String(body)
    }
}
