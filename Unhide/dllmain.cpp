//UNHIDER Dll Code
#define WIN32_LEAN_AND_MEAN

#include "Windows.h"

// Method to Unhide from Screenshare
void setDAForWindows() {
	HWND windowHandle = NULL;
	do {
		windowHandle = FindWindowEx(NULL, windowHandle, NULL, NULL);
		if (windowHandle) {
			// Restore normal display affinity
			SetWindowDisplayAffinity(windowHandle, WDA_NONE);
		}
	} while (windowHandle);
}


// Method to Unhide/Restore Taskbar and Alt-Tab Task Switcher visibility
void restoreStyleForWindows() {
	// Get the window handle of the current process
	DWORD currentProcessId = GetCurrentProcessId();
	HWND windowHandle = NULL;

	do {
		windowHandle = FindWindowEx(NULL, windowHandle, NULL, NULL);
		if (windowHandle) {
			DWORD processId = 0;
			GetWindowThreadProcessId(windowHandle, &processId);

			// Only modify windows from the current process
			if (processId == currentProcessId) {
				// Restore normal window styles (remove TOOLWINDOW, add APPWINDOW)
				LONG_PTR style = GetWindowLongPtr(windowHandle, GWL_EXSTYLE);
				style &= ~WS_EX_TOOLWINDOW;  // Remove tool window style
				style |= WS_EX_APPWINDOW;    // Add app window style
				SetWindowLongPtr(windowHandle, GWL_EXSTYLE, style);
			}
		}
	} while (windowHandle);
}


BOOL APIENTRY DllMain(HMODULE hModule,
	DWORD  ul_reason_for_call,
	LPVOID lpReserved
)
{
	switch (ul_reason_for_call)
	{
	case DLL_PROCESS_ATTACH:
		setDAForWindows();
		restoreStyleForWindows();
		break;
	case DLL_THREAD_ATTACH:
	case DLL_THREAD_DETACH:
	case DLL_PROCESS_DETACH:
		break;
	}
	return FALSE; 
}

