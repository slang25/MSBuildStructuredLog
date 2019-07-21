using Avalonia;
using System;
using System.Runtime.InteropServices;
using Avalonia.Gtk3;

namespace StructuredLogViewer.Avalonia
{
    class Program
    {
        static void Main(string[] args)
        {
            ExceptionHandler.Initialize();
            //DialogService.ShowMessageBoxEvent += message => MessageBox.Show(message);
            ClipboardService.Set += text => Application.Current.Clipboard.SetTextAsync(text);

            AppBuilder.Configure<App>()
                .UsePlatformDetect()
                .With(new AvaloniaNativePlatformOptions { UseGpu = true })
                .With(new MacOSPlatformOptions { ShowInDock = true })
                .Start<MainWindow>();
        }
    }
}
