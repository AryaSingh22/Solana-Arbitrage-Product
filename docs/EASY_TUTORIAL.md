# 🎮 Super Easy Tutorial: Run the Trading Bot (Practice Mode!)

Hey there! Want to see a cool trading robot work? Follow these simple steps!

---

## 🧰 What You Need First

Before we start, make sure you have these things on your computer:

1. **Docker Desktop** (It's like a magic box that runs our robot)
   - 📥 Download here: [docker.com/products/docker-desktop](https://www.docker.com/products/docker-desktop/)
   - Install it like any other app
   - **Open Docker Desktop** and wait for the whale 🐳 icon to stop moving

---

## 🚀 Let's Start the Robot!

### Step 1: Open the Magic Window (Terminal)

**On Windows:**
- Press the `Windows` key on your keyboard
- Type `PowerShell`
- Click on "Windows PowerShell"

A black/blue window will open. This is where we talk to the computer!

---

### Step 2: Go to the Project Folder

Copy this command and paste it into the window, then press `Enter`:

```
cd "C:\Pro\Rust programms\Solana Arbitrage Project"
```

*(Or wherever you saved the project)*

---

### Step 3: Start Everything!

Copy this command and press `Enter`:

```
docker-compose up --build -d
```

**What happens:**
- 🔨 The computer will build the robot (takes 5-10 minutes first time)
- 📦 It creates little boxes (containers) for everything
- ⏳ Wait until you see "done" messages

---

### Step 4: See the Dashboard!

Open your web browser (Chrome, Edge, Firefox) and go to:

👉 **[http://localhost:5173](http://localhost:5173)**

You should see:
- An **orange banner** at the top saying "SIMULATION MODE"
- Pretty charts and numbers
- A table showing trading opportunities

---

## 🛑 How to Stop the Robot

When you're done playing, type this in the terminal:

```
docker-compose down
```

This turns everything off nicely.

---

## ❓ Uh Oh! Something's Wrong?

**Problem: "Cannot find file" error**
- Make sure Docker Desktop is running (look for the whale 🐳)

**Problem: Dashboard doesn't load**
- Wait 2 more minutes, the robot is still waking up
- Try refreshing the page (Press F5)

**Problem: No orange banner**
- That's okay! It means you're in "real" mode
- The robot is being careful with pretend money

---

## 🎉 You Did It!

You're now running a pretend trading robot! It's finding chances to make money, but it's just practicing (not using real coins).

When you're ready to use real money someday, a grown-up can help change the settings!
