# Invisiwind

Invisiwind (short for *Invisible Window*) is an application that allows you to hide certain windows when sharing your full screen.

### So .. what does it do exactly?

I think this is best explained with a couple of screenshots:

<p float="left">
  <img src="./demo/here.png" width="400" alt="What I see" />
  <img src="./demo/there.png" width="400" alt="What they see" />
</p>

Using this tool, firefox and slack have been hidden so anyone watching the screenshare is unable to see those windows. However, I can continue to use them as usual on my side.

### So .. what does it do exactly? (for technical people)

Simple: The tool performs dll injection to [SetWindowDisplayAffinity](https://docs.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity) to `WDA_EXCLUDEFROMCAPTURE`.

### How do I use it?

 - Download and extract the zip bundle from here.
 - Run `Invisiwind.exe`. You will now be dropped into a terminal.