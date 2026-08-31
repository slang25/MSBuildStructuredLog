using System;
using System.Runtime.InteropServices;
using System.Text;

namespace StructuredLogViewer.NativeBridge;

/// <summary>
/// UTF-8 string marshaling helpers for the C ABI. Every string returned
/// through an out-parameter is allocated here and must be released by the
/// caller via <c>mslog_string_free</c> (which calls <see cref="Free"/>).
/// </summary>
internal static unsafe class NativeStrings
{
    /// <summary>Allocates a null-terminated UTF-8 copy of <paramref name="value"/>.</summary>
    public static IntPtr ToNative(string value)
    {
        if (value == null)
        {
            return IntPtr.Zero;
        }

        int byteCount = Encoding.UTF8.GetByteCount(value);
        byte* buffer = (byte*)NativeMemory.Alloc((nuint)byteCount + 1);
        fixed (char* chars = value)
        {
            Encoding.UTF8.GetBytes(chars, value.Length, buffer, byteCount);
        }

        buffer[byteCount] = 0;
        return (IntPtr)buffer;
    }

    public static void Free(IntPtr ptr)
    {
        if (ptr != IntPtr.Zero)
        {
            NativeMemory.Free((void*)ptr);
        }
    }

    /// <summary>Reads a null-terminated UTF-8 string owned by the caller.</summary>
    public static string FromNative(IntPtr ptr) => Marshal.PtrToStringUTF8(ptr);
}
