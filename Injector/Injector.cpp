// Injector.cpp : This file contains the 'main' function. Program execution begins and ends there.
//

#include <iostream>
#include <iomanip>
#include <string>
#include <map>
#include <unordered_set>
#include <algorithm>
#include <Windows.h>
#include <TlHelp32.h>
#include <AclAPI.h>

using std::cout;
using std::cerr;
using std::endl;

#ifdef _WIN64
const bool iAm64bit = true;
const std::wstring hideDllName{ L"./Hide.dll" };
const std::wstring unhideDllName{ L"./Unhide.dll" };
#else
const bool iAm64bit = false;
const std::wstring hideDllName{ L"./Hide_32bit.dll" };
const std::wstring unhideDllName{ L"./Unhide_32bit.dll" };
#endif

const std::wstring exeName{ L"./Invisiwind.exe" };
const std::wstring exeName32{ L"./Invisiwind_32bit.exe" };
const std::wstring title{ L"  _____            _     _          _           _ \n"
"  \\_   \\_ ____   _(_)___(_)_      _(_)_ __   __| |\n"
"   / /\\/ '_ \\ \\ / / / __| \\ \\ /\\ / / | '_ \\ / _` |\n"
"/\\/ /_ | | | \\ V /| \\__ \\ |\\ V  V /| | | | | (_| |\n"
"\\____/ |_| |_|\\_/ |_|___/_| \\_/\\_/ |_|_| |_|\\__,_|" };

void showHelp(const wchar_t* argZero) {
	std::wcout << "Invisiwind - Hide certain windows from screenshares.\n"
		"\n"
		"Usage: " << argZero << " [--hide | --unhide] PID_OR_PROCESS_NAME ...\n"
		"\n"
		"  -h, --hide      Hide the specified applications. This is default.\n"
		"  -u, --unhide    Unhide the applications specified.\n"
		"      --help      Show this help menu.\n"
		"\n"
		"  PID_OR_PROCESS_NAME The process id or the process name to hide.\n"
		"\n"
		"Examples:\n"
		<< argZero << " 89203\n"
		<< argZero << " firefox.exe\n"
		<< argZero << " --unhide discord.exe obs64.exe\n";
}

// Most functions have two types - A (ANSI - old) and W (Unicode - new)

// Why do some functions have Ex? This is a new version of the function that has a different API to accomodate new features,
// but has an Ex to prevent breaking old code.

std::unordered_set<int> getPIDsFromProcName(std::wstring& searchTerm) {
	std::unordered_set<int> pids;
	std::transform(searchTerm.begin(), searchTerm.end(), searchTerm.begin(), ::towlower);
	HANDLE hSnapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
	if (hSnapshot) {
		PROCESSENTRY32 pe32{};
		pe32.dwSize = sizeof(PROCESSENTRY32);
		if (Process32First(hSnapshot, &pe32)) {
			do {
				std::wstring exeFile{ pe32.szExeFile };
				std::transform(exeFile.begin(), exeFile.end(), exeFile.begin(), ::towlower);
				if (searchTerm == exeFile)
					pids.insert(pe32.th32ProcessID);
			} while (Process32Next(hSnapshot, &pe32));
		}
		CloseHandle(hSnapshot);
	}
	return pids;
}

std::map<std::wstring, std::unordered_set<int>> getProcList() {
	std::map<std::wstring, std::unordered_set<int>> pList;
	HANDLE hSnapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
	if (hSnapshot) {
		PROCESSENTRY32 pe32{};
		pe32.dwSize = sizeof(PROCESSENTRY32);
		if (Process32First(hSnapshot, &pe32)) {
			do {
				pList[pe32.szExeFile].insert(pe32.th32ProcessID);
			} while (Process32Next(hSnapshot, &pe32));
		}
		CloseHandle(hSnapshot);
	}
	return pList;
}

bool isValidPID(std::wstring& arg) {
	if (arg.empty()) return false;
	for (auto& c : arg)
		if (!isdigit(c))
			return false;
	return true;
}

// From https://stackoverflow.com/questions/3828835/how-can-we-check-if-a-file-exists-or-not-using-win32-program
// szPath - String Zero-terminated Path
bool FileExists(std::wstring& filePath)
{
	// https://docs.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfileattributesw
	DWORD dwAttrib = GetFileAttributes(filePath.c_str());

	return (dwAttrib != INVALID_FILE_ATTRIBUTES &&
		!(dwAttrib & FILE_ATTRIBUTE_DIRECTORY));
}

std::wstring getFullFilePath(const std::wstring& filename) {
	wchar_t fullPath[1024];
	// DWORD GetFullPathNameW(
	// 	LPCWSTR lpFileName,    - Relative path
	// 	DWORD   nBufferLength, - Buffer size
	// 	LPWSTR  lpBuffer,      - Buffer
	// 	LPWSTR *lpFilePart     - Pointer to the filename part
	// );
	GetFullPathName(filename.c_str(), 256, fullPath, NULL);
	std::wstring strFullPath{ fullPath };
	if (!FileExists(strFullPath)) {
		std::wcerr << "WARNING:" << strFullPath << " not found.";
		return std::wstring{};
	}
	return strFullPath;
};

// LP  - Pointer
// C   - Constant
// T   - TCHAR (or W - WCHAR)
// STR - String

int wmain(int argc, wchar_t* argv[], wchar_t* envp[])
{
	// Check if DLL exists and then store path
	std::wstring hideDllPath{ getFullFilePath(hideDllName) }, unhideDllPath{ getFullFilePath(unhideDllName) };

	// Check if 32-bit version of the executable exists
	std::wstring x86binPath{ getFullFilePath(exeName32) };
	bool x86verExists = x86binPath.length();

	auto inject = [&](DWORD pid, std::wstring& dllFullPath) -> void {
		if (!dllFullPath.length()) return;
		size_t dllPathLen = (dllFullPath.length() + 1) * sizeof(wchar_t);
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
					cliargs += exeName32;
					cliargs += L" ";
					if (dllFullPath == unhideDllPath) cliargs += L"-u ";
					cliargs += std::to_wstring(pid);

					if (CreateProcess(x86binPath.c_str(), &cliargs[0], NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi)) {
						WaitForSingleObject(pi.hProcess, INFINITE);
						CloseHandle(pi.hProcess);
						CloseHandle(pi.hThread);
					}
					else cerr << "Unknown error occurred when trying to run 32-bit exe." << endl;
				}
				else std::wcerr << "Cannot hide 32-bit process " << pid << " since " << exeName32 << " is missing." << endl;
				CloseHandle(procHandle);
				return;
			}
			// BOOL GetModuleHandleExW(
			// 	DWORD   dwFlags,        - Some random flags you can pass in
			// 	LPCWSTR lpModuleName,   - Module name
			// 	HMODULE *phModule       - Pointer to module handle if successful
			// );
			if (HMODULE libHandle; GetModuleHandleEx(0, L"kernel32.dll", &libHandle)) {
				// cerr << "Kernel32.dll base address is " << libHandle << endl;
				// FARPROC GetProcAddress(
				// 	HMODULE hModule,    - Module handle
				// 	LPCSTR  lpProcName  - Library/Variable you want the address of
				// );
				if (LPVOID libAddr = GetProcAddress(libHandle, "LoadLibraryW"); libAddr) {
					// cerr << "Library Address at " << libAddr << endl;
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
						if (WriteProcessMemory(procHandle, mem, dllFullPath.c_str(), dllPathLen, NULL)) {
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
							// cerr << "Successfully wrote dllFullPath: ";
							// std::wcerr << std::wstring(dllFullPath) << endl;
							if (HANDLE remoteThread = CreateRemoteThreadEx(procHandle, NULL, 0, static_cast<LPTHREAD_START_ROUTINE>(libAddr), mem, 0, NULL, NULL); remoteThread) {
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
	};

	if (argc > 1) {
		bool hide = true;
		for (int i = 1; i < argc; i++) {
			std::wstring arg{ argv[i] };
			std::transform(arg.begin(), arg.end(), arg.begin(), ::towlower);
			if ((arg == L"-h" and argc == 2) or arg == L"--help" or arg == L"/?") {
				showHelp(argv[0]);
				return 0;
			}
			else if (arg == L"-h" or arg == L"--hide") {
				hide = true;
			}
			else if (arg == L"-u" or arg == L"--unhide") {
				hide = false;
			}
			else if (isValidPID(arg)) {
				inject(stoi(arg), hide ? hideDllPath : unhideDllPath);
			}
			else {
				auto pids = getPIDsFromProcName(arg);

				// If we find no results, append .exe and try again
				if (pids.empty()) pids = getPIDsFromProcName(arg.append(L".exe"));

				if (pids.empty())
					std::wcerr << L"No process found with the name " << argv[i] << endl;
				for (auto& pid : pids)
					inject(pid, hide ? hideDllPath : unhideDllPath);
			}
		}
		return 0;
	}

	// Interactive mode
	std::wcout << title << endl;
	std::wcout << "Hey I'm Invisiwind, here to make windows invisible to everyone but you ^_^" << endl;
	std::wcout << "Type `help` to get started." << endl;

	int enterPressed{};
	while (true) {
		std::wstring input;
		cout << "> ";
		getline(std::wcin, input);
		if (input.empty()) {
			if (enterPressed++) {
				cout << "Exiting .. Have a great day!";
				return 0;
			}
			cout << "Press Enter again to exit" << endl;
		}
		else {
			enterPressed = 0;

			auto delimPos = input.find(L" ");
			std::wstring command = input.substr(0, delimPos);

			if (command == L"help" or command == L"`help`") {
				std::cout << "Available commands: \n"
					"\n"
					"  hide PROCESS_ID_OR_NAME       Hides the specified application\n"
					"  unhide PROCESS_ID_OR_NAME     Unhides the specified application\n"
					"  list                          Lists all applications\n"
					"  help                          Shows this help menu\n"
					"  exit                          Exit\n"
					"\n"
					"Examples:\n"
					"hide notepad.exe\n"
					"list\n"
					"unhide discord.exe\n";
			}
			else if (command == L"list") {
				std::wcout << std::setw(35) << std::left << "Process name" << "PID" << endl;
				for (auto& [pName, pIDs] : getProcList()) {
					std::wcout << std::setw(35) << std::left << pName;
					for (auto& pID : pIDs) std::cout << pID << " ";
					cout << endl;
				}
			}
			else if (command == L"hide" or command == L"unhide") {
				if (delimPos == std::wstring::npos) {
					std::wcout << "Usage: " << command << " PROCESS_ID_OR_NAME\n";
					continue;
				}
				std::wstring arg = input.substr(delimPos + 1);
				if (isValidPID(arg)) {
					inject(stoi(arg), command == L"hide" ? hideDllPath : unhideDllPath);
				}
				else {
					auto pids = getPIDsFromProcName(arg);

					// If we find no results, append .exe and try again
					if (pids.empty()) pids = getPIDsFromProcName(arg.append(L".exe"));

					if (pids.empty())
						std::wcerr << L"No process found with the name " << input.substr(delimPos + 1) << endl;
					for (auto& pid : pids)
						inject(pid, command == L"hide" ? hideDllPath : unhideDllPath);
				}
			}
			else if (command == L"exit" or command == L"quit") {
				cout << "Exiting .. have a good day!\n";
				return 0;
			}
			else {
				cout << "Invalid command. Type `help` for help." << endl;
			}
		}
	}

	return 0;
}
