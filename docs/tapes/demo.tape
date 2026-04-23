Output docs/public/demo.gif

Set Shell "bash"
Set FontSize 22
Set FontFamily "JetBrains Mono"
Set Width 900
Set Height 500
Set Padding 20
Set Margin 0
Set MarginFill "#ffffff"
Set BorderRadius 2
Set WindowBar Colorful
Set WindowBarSize 40
Set TypingSpeed 80ms
Set Framerate 24
Set CursorBlink false

Set Theme {"name":"dbmcp","black":"#1a1a1a","red":"#ff4800","green":"#16a34a","yellow":"#d97706","blue":"#2563eb","magenta":"#9333ea","cyan":"#0891b2","white":"#374151","brightBlack":"#6b7280","brightRed":"#ff6b35","brightGreen":"#22c55e","brightYellow":"#f59e0b","brightBlue":"#3b82f6","brightMagenta":"#a855f7","brightCyan":"#06b6d4","brightWhite":"#111827","background":"#ffffff","foreground":"#1a1a1a","selection":"#ff480020","cursor":"#ff4800"}

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
