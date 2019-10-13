using System;
using System.Collections.Generic;
using System.Globalization;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Shapes;
using Avalonia.Data.Converters;
using Avalonia.Media;
using Microsoft.Build.Logging.StructuredLogger;

namespace StructuredLogViewer.Avalonia.Controls
{
    public class ProxyNodeIconConverter : IValueConverter
    {
        private readonly Dictionary<string, object> resources = new Dictionary<string, object>();
        private readonly ProjectIconConverter projectIconConverter = new ProjectIconConverter();

        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            var node = value as ProxyNode;
            if (node == null)
                return null;

            switch (node.OriginalType)
            {
                case nameof(Build):
                case nameof(Property):
                    return NodeIcon("PropertyStroke", "PropertyBrush");
                case nameof(Folder):
                    return NodeIcon("FolderStroke", "ClosedFolderBrush");
                case nameof(Target):
                    return NodeIcon("TargetStroke", "TargetBrush");
                case nameof(Task):
                    return NodeIcon("TaskStroke", "TaskBrush");
                case nameof(AddItem):
                    return NodeIcon("ItemStroke", "ItemBrush");
                case nameof(RemoveItem):
                    return NodeIcon("ItemStroke", "ItemBrush");
                case nameof(Item):
                    return NodeIcon("ItemStroke", "ItemBrush");
                case nameof(Metadata):
                    return NodeIcon("MetadataStroke", "ItemBrush");
                case nameof(Parameter):
                    return NodeIcon("ParameterStroke", "ParameterBrush");
                case nameof(Message):
                    return NodeIcon("MessageStroke", "MessageBrush");
                case nameof(Error):
                    return NodeIcon("ErrorStroke", "ErrorBrush");
                case nameof(Warning):
                    return NodeIcon("WarningStroke", "WarningBrush");
                case nameof(Import):
                    return NodeIcon("ImportStroke", "ImportBrush");
                case nameof(NoImport):
                    return NodeIcon("NoImportStroke", "NoImportBrush");
                case nameof(Project):
                    return ProjectIcon(node.ProjectExtension);
                default:
                    return NodeIcon(null, null);
            }
        }

        private Rectangle NodeIcon(string stroke, string fill)
        {
            return new Rectangle
            {
                Width = 14,
                Height = 11,
                Margin = new Thickness(3, 5, 6, 3),
                Stroke = GetResource(stroke) as IBrush,
                StrokeThickness = 1,
                Fill = GetResource(fill) as IBrush,
                VerticalAlignment = VerticalAlignment.Top
            };
        }

        private object ProjectIcon(string projectExtension)
        {
            return new DrawingPresenter
            {
                Width = 16,
                Height = 16,
                Margin = new Thickness(2, 4, 6, 2),
                Drawing = projectIconConverter.ProjectExtensionToIcon(projectExtension),
                VerticalAlignment = VerticalAlignment.Top
            };
        }
        
        private object GetResource(string resourceName)
        {
            if (string.IsNullOrEmpty(resourceName))
                return null;
            
            if (!resources.TryGetValue(resourceName, out var resource))
            {
                if (!Application.Current.Resources.TryGetResource(resourceName, out resource))
                    resource = null;

                resources[resourceName] = resource;
            }

            return resource;
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) 
            => throw new NotSupportedException();
    }
}
