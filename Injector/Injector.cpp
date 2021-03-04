// Injector.cpp : This file contains the 'main' function. Program execution begins and ends there.
//

#include <iostream>
#include <string>
#include <unordered_set>
#include <Windows.h>
#include <TlHelp32.h>

using std::cout;
using std::cerr;
using std::endl;

#ifdef _WIN64
const bool iAm64bit = true;
#else
const bool iAm64bit = false;
#endif

// Most functions have two types - A (ANSI - old) and W (Unicode - new)

// Why do some functions have Ex? This is a new version of the function that has a different API to accomodate new features,
// but has an Ex to prevent breaking old code.


// From https://stackoverflow.com/questions/3828835/how-can-we-check-if-a-file-exists-or-not-using-win32-program
// szPath - String Zero-terminated Path
bool FileExists(LPCWSTR szPath)
{
	// https://docs.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfileattributesw
	DWORD dwAttrib = GetFileAttributes(szPath);

	return (dwAttrib != INVALID_FILE_ATTRIBUTES &&
		!(dwAttrib & FILE_ATTRIBUTE_DIRECTORY));
}

int savePIDsFromProcName(std::unordered_set<int>& pids, std::wstring& searchTerm) {
	int found = 0;
	HANDLE hSnapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
	if (hSnapshot) {
		PROCESSENTRY32 pe32;
		pe32.dwSize = sizeof(PROCESSENTRY32);
		if (Process32First(hSnapshot, &pe32)) {
			do {
				if (searchTerm == pe32.szExeFile) {
					found++;
					pids.insert(pe32.th32ProcessID);
				}
			} while (Process32Next(hSnapshot, &pe32));
		}
		CloseHandle(hSnapshot);
	}
	return found;
}

bool isValidPID(std::wstring& arg) {
	for (auto& c: arg)
		if (!isdigit(c))
			return false;
	return true;
}

// LP  - Pointer
// C   - Constant
// T   - TCHAR (or W - WCHAR)
// STR - String

int wmain(int argc, wchar_t* argv[], wchar_t* envp[])
{
	std::unordered_set<int> pids;
	if (argc > 1) {
		for (int i = 1; i < argc; i++) {
			std::wstring arg = argv[i];
			if (isValidPID(arg))
				pids.insert(_wtoi(argv[i]));
			else if (!savePIDsFromProcName(pids, arg))
				std::wcerr << L"No process found with the name " << arg << endl;
		}
	}

	// DWORD GetFullPathNameW(
	// 	LPCWSTR lpFileName,    - Relative path
	// 	DWORD   nBufferLength, - Buffer size
	// 	LPWSTR  lpBuffer,      - Buffer
	// 	LPWSTR *lpFilePart     - Pointer to the filename part
	// );
	TCHAR dllFullPath[256], * fileExt;
	auto dllName = iAm64bit ? L"./Payload.dll" : L"./Payload_32bit.dll";
	GetFullPathName(dllName, 256, dllFullPath, &fileExt);

	if (!FileExists(dllFullPath)) {
		std::wcerr << dllName << " not found.";
		return 1;
	}

	TCHAR x86binPath[256];
	GetFullPathName(L"./Windoer_32bit.exe", 256, x86binPath, NULL);
	bool x86verExists = FileExists(x86binPath);

	size_t dllPathLen = (wcslen(dllFullPath) + 1) * sizeof(TCHAR);

	if (pids.empty()) {
		cerr << "No pids specified" << endl;
		return 1;
	}

	for (auto& pid : pids) {

		// HANDLE OpenProcess(
		// 	DWORD dwDesiredAccess,
		// 	BOOL  bInheritHandle,       - Whether child processes should inherit this handle
		// 	DWORD dwProcessId		   
		// );						   
		// PROCESS_CREATE_THREAD        - Create thread
		// PROCESS_QUERY_INFORMATION    - To query information about the process like exit code, priority, etc (do we need this?)
		// PROCESS_VM_READ              - To read process memory
		// PROCESS_VM_WRITE             - To write process memory
		// PROCESS_VM_OPERATION         - To perform an operation on the memory (needed for writing)
		if (HANDLE procHandle = OpenProcess(PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION, false, pid); procHandle) {
			cerr << "Opened handle for pid " << (DWORD)pid;
			if (!iAm64bit) cerr << " (32 bit)";
			cerr << endl;

			if (BOOL procIs32bit; iAm64bit and IsWow64Process(procHandle, &procIs32bit) and procIs32bit) {
				if (x86verExists) {
					STARTUPINFO si;
					PROCESS_INFORMATION pi;

					ZeroMemory(&si, sizeof(si));
					si.cb = sizeof(si);
					ZeroMemory(&pi, sizeof(pi));

					std::wstring cliargs;
					cliargs += L"Windoer_32bit.exe ";
					cliargs += std::to_wstring(pid);

					if (CreateProcess(x86binPath, &cliargs[0], NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi)) {
						WaitForSingleObject(pi.hProcess, INFINITE);
						CloseHandle(pi.hProcess);
						CloseHandle(pi.hThread);
					}
					else cerr << "Unknown error occurred when trying to run 32-bit exe." << endl;
				}
				else cerr << "Cannot hide 32-bit process " << pid << " since Windoer_32bit.exe is missing." << endl;
				CloseHandle(procHandle);
				continue;
			}
			// BOOL GetModuleHandleExW(
			// 	DWORD   dwFlags,        - Some random flags you can pass in
			// 	LPCWSTR lpModuleName,   - Module name
			// 	HMODULE *phModule       - Pointer to module handle if successful
			// );
			if (HMODULE libHandle; GetModuleHandleEx(0, L"kernel32.dll", &libHandle)) {
				// FARPROC GetProcAddress(
				// 	HMODULE hModule,    - Module handle
				// 	LPCSTR  lpProcName  - Library/Variable you want the address of
				// );
				if (LPVOID libAddr = GetProcAddress(libHandle, "LoadLibraryW"); libAddr) {
					cerr << "Library Address at " << libAddr << endl;
					// LPVOID VirtualAllocEx(    
					// 	HANDLE hProcess,         - Process handle
					// 	LPVOID lpAddress,        - The starting address of where we want to start allocating
					// 	SIZE_T dwSize,           - The amount we want to allocate (length + 1 null byte)
					// 	DWORD  flAllocationType, - Allocation type
					// 	DWORD  flProtect         - Protections
					// );
					if (LPVOID mem = VirtualAllocEx(procHandle, NULL, dllPathLen, MEM_COMMIT, PAGE_READWRITE); mem) {
						// BOOL WriteProcessMemory(
						// 	HANDLE  hProcess,
						// 	LPVOID  lpBaseAddress,           - where to start writing from
						// 	LPCVOID lpBuffer,                - what to write
						// 	SIZE_T  nSize,                   - how much to write
						// 	SIZE_T * lpNumberOfBytesWritten  - a pointer to store how many bytes were written
						// );
						if (WriteProcessMemory(procHandle, mem, dllFullPath, dllPathLen, NULL)) {
							// HANDLE CreateRemoteThreadEx(
							// 	HANDLE                       hProcess,
							// 	LPSECURITY_ATTRIBUTES        lpThreadAttributes, - Security attributes
							// 	SIZE_T                       dwStackSize,        - Stack size (0 - default)
							// 	LPTHREAD_START_ROUTINE       lpStartAddress,     - What address to start thread
							// 	LPVOID                       lpParameter,        - a parameter to pass to the above function (in our case, dll path)
							// 	DWORD                        dwCreationFlags,    - flags such as whether to start suspended, etc
							// 	LPPROC_THREAD_ATTRIBUTE_LIST lpAttributeList,
							// 	LPDWORD                      lpThreadId
							// );
							if (HANDLE remoteThread = CreateRemoteThreadEx(procHandle, NULL, 0, (LPTHREAD_START_ROUTINE)libAddr, mem, 0, NULL, NULL); remoteThread) {
								// If we wanted to wait for the dll thread to return
								// WaitForSingleObject(remoteThread, INFINITE);
								if (CloseHandle(remoteThread) and CloseHandle(procHandle))
									cerr << "Success!" << endl;
								else cerr << "Injected Dll, but failed to close handles" << endl;
							}
							else cerr << "Failed to create remote thread" << endl;
						}
						else cerr << "Failed to write to allocated memory" << endl;
					}
					else cerr << "Failed to allocate memory" << endl;
				}
				else cerr << "Failed to get address of LoadLibraryW" << endl;
			}
			else cerr << "Failed to acquire handle on kernel32.dll" << endl;
		}
		else cerr << "Failed to acquire handle on process " << pid << endl;
	}
	return 0;
}

