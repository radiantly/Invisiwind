//HIDER Dll Code
#define WIN32_LEAN_AND_MEAN

#include "Windows.h"

//Method to Hide from Screenshare
void setDAForWindows() {
	HWND windowHandle = NULL;
	do {
		windowHandle = FindWindowEx(NULL, windowHandle, NULL, NULL);
		if (windowHandle) {
			// Hide from screen capture
			SetWindowDisplayAffinity(windowHandle, WDA_EXCLUDEFROMCAPTURE);
		}
	} while (windowHandle);
}

//Method to Hide from Taskbar and Alt-Tab Task Switcher
void setStyleForWindows() {
	// Get the window handle of the current process
	DWORD currentProcessId = GetCurrentProcessId();
	HWND currentWindow = NULL;

	// Find windows belonging to the current process
	HWND windowHandle = NULL;
	do {
		windowHandle = FindWindowEx(NULL, windowHandle, NULL, NULL);
		if (windowHandle) {
			DWORD processId = 0;
			GetWindowThreadProcessId(windowHandle, &processId);

			// Only modify windows from the current process
			if (processId == currentProcessId) {
				// Set window styles (add TOOLWINDOW, remove APPWINDOW)
				LONG_PTR style = GetWindowLongPtr(windowHandle, GWL_EXSTYLE);
				style &= ~WS_EX_APPWINDOW;  // Remove app window style
				style |= WS_EX_TOOLWINDOW;  // Add tool window style
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
		setStyleForWindows();
		break;
	case DLL_THREAD_ATTACH:
	case DLL_THREAD_DETACH:
	case DLL_PROCESS_DETACH:
		break;
	}
	return FALSE;
}

