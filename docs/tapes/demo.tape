Output docs/public/demo.gif

Set Shell "bash"
Set FontSize 22
Set FontFamily "JetBrains Mono"
Set Width 900
Set Height 500
Set Padding 20
Set Margin 0
Set MarginFill "#01112c"
Set BorderRadius 0
Set WindowBar Colorful
Set WindowBarSize 40
Set TypingSpeed 80ms
Set Framerate 24
Set CursorBlink false

Set Theme {"name":"dbmcp","black":"#01112c","red":"#ff8389","green":"#42be65","yellow":"#f1c21b","blue":"#78a9ff","magenta":"#8f8afb","cyan":"#3ddbd9","white":"#c9d1e0","brightBlack":"#9aa6bc","brightRed":"#ffb3b8","brightGreen":"#6fdc8c","brightYellow":"#fddc69","brightBlue":"#a6c8ff","brightMagenta":"#b9b5ff","brightCyan":"#9ef0f0","brightWhite":"#e6e9ef","background":"#01112c","foreground":"#e6e9ef","selection":"#625bf840","cursor":"#8f8afb"}

# Hidden setup: launch interactive simulation
Hide
Type "cd docs/tapes && ./demo.sh"
Enter
Sleep 1s
Show

# Query 1: Count users (think 1.8s + read answer)
Sleep 300ms
Type "How many users are in the database?"
Sleep 500ms
Enter
Sleep 5s

# Query 2: Most active author (think 3.2s + read answer)
Type "Which user wrote the most posts?"
Sleep 500ms
Enter
Sleep 6s

# Query 3: Recent posts (think 2.4s + read answer)
Type "Show me the most recent posts"
Sleep 500ms
Enter
Sleep 6s
