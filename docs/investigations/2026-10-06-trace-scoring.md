# Scoring pedal traces and holds (SCT-032/036 research)
Date: 2026-10-06


> **Editor's note (Claude):** this document was drafted by Gemini from web research and reviewed by Claude. A spot-check of the product claims against brakinglab.com found three errors (exercise count, a scoring split and a tolerance range), which are corrected or marked Unverified here. The other product and feature claims in section 1 weren't checked individually, so read them as Unverified. The motor-control sections (2–6) cite standard literature and match it as far as Claude can tell. The formulas in section 7 are proposals for SCT-036, not decisions.
## TL;DR
- Standalone pedal trainers evaluate accuracy via Time on Target or RMSE, but commercial telemetry platforms evaluate driving in-sim rather than isolated off-track motor tracking.
- Classical motor-control tracking metrics (RMSE, Time in Band, cross-correlation lag) separate spatial tracking errors from temporal phase shifts.
- Modern motor smoothness is best quantified by Log Dimensionless Jerk (LDLJ) or Spectral Arc Length (SPARC), which remain invariant to movement duration and amplitude.
- Calculating jerk from 1 kHz pedal data requires low-pass filtering at 10–12 Hz to suppress differentiation-induced high-frequency quantization noise without blunting human motor transients.
- Bandwidth feedback theory demonstrates that a multi-tier tolerance band (±8% Rookie, ±6% Clubman, ±4% Pro) maximizes motor learning by preventing cognitive overload and feedback dependency.

## 1. Existing trainers

### 1.1 Commercial and web pedal trainers
Several tools offer isolated pedal drills or brake analysis, though nearly all run in web browsers using the Gamepad API:

- **Braking Lab** ([brakinglab.com](https://www.brakinglab.com/en)): A web-based sim-racing training platform with a companion desktop application ("Braking Lab Capture") that monitors local directories for iRacing `.ibt` and Le Mans Ultimate telemetry files. Braking Lab offers a "Brake Master" module; its site lists "120+" exercises over 11 tracks and 5 licences (Rookie to A), checked 2026-10-06.
  - *Tolerance Band:* The platform implements a configurable "Exercise Tolerance" band ranging between 3% and 20%, recommending 15% for noisy real-world telemetry ([brakinglab.com changelog](https://www.brakinglab.com/en)).
  - *Trail-Braking Scoring:* **Unverified.** The draft claimed a 70% release smoothness / 30% peak hold split; the Braking Lab site does not document its scoring formula.
  - *Gamification and Feedback:* Exercises display target traces with live tolerance boundaries. Ratings use star thresholds (e.g., 70+, 85+, 92+) and assign licenses progressing from Rookie to Pro.
- **Baseline Driver Training** ([baselinedrivertraining.dk](https://baselinedrivertraining.dk/)): A native Windows subscription desktop application (€19/month or €119/year) that isolates brake and throttle pedal practice. It provides readouts in kilograms of force and percentage travel. It structures learning into four phases: Foundation, Consistency, Speed, and Mastery. Performance is tracked via global cloud leaderboards, though its underlying scoring equations are proprietary and closed-source.
- **Trail Braking Trainer** ([trailbraking.uk](https://trailbraking.uk/)): A free browser-based trainer utilizing the Gamepad API. Users follow a target ghost brake trace constrained by tolerance rails across preset corner profiles (hairpin, 90-degree corner, sweeper).
  - *Scoring:* Evaluates performance using a "repeatability score" calculated from the Root Mean Square Error (RMSE) between user pedal input and the reference profile ([trailbraking.uk on Reddit](https://www.reddit.com/r/simracing/comments/182k79s/free_browser_tool_to_practise_trail_braking/)).
- **SD70 BrakeMaster** ([brakemaster.live](https://brakemaster.live/)): A free web tool supporting pressure hold, trail braking, consistency, reaction time, and "blind" muscle-memory drills. Displays live input curves and logs personal and global best records.
- **SIXTYCM** ([sixtycm.com](https://sixtycm.com/)): A browser-based motor-control trainer offering "Precision Tunnel", "Target Gates", and "Brake Release" drills across Learn, Recall, and Blind modes. It includes an adaptive difficulty algorithm and input smoothing for coarse potentiometer pedal hardware.
- **HSimRacing Brake Curve Analyzer** ([hsimracing.com](https://hsimracing.com/brake-curve-analyzer/)): A web-based diagnostic tool evaluating a single brake application across four sub-scores (0–100):
  1. *Shape:* Geometric conformity to an ideal deceleration curve.
  2. *Smoothness:* Fluidity and linearity during pressure application and release.
  3. *Attack:* Rate of rise to peak threshold pressure.
  4. *Control:* Modulation stability during trail-off.
- **VPS Brake Pressure Sounder** ([velocityprosims.com](https://www.velocityprosims.com/pages/brake-pressure-sounder-trainer)): A web audio utility that emits a tone when brake input crosses a target percentage, incorporating hysteresis to prevent rapid audio flutter.

### 1.2 Hardware utility software
- **Simucube ActivePedal / Simucube Tuner** ([simucube.com](https://simucube.com/activepedal/)): Active force-feedback pedal software allows motorized configuration of preload, travel, end-stops, damping, and non-linear force curves (Linear, Squared, S-curve, Logarithmic). It generates ABS and traction loss vibrations from simulator telemetry. It does not provide target tracking drills or user scoring algorithms.
- **Fanatec Fanalab / Control Panel** ([fanatec.com](https://fanatec.com/)): Provides hardware-level calibration for load cells and potentiometers, deadzones, Brake Force (BRF) sensitivity, and ABS motor vibration thresholds. It lacks drill modes or tracing analysis.

### 1.3 Telemetry and coaching platforms
- **Coach Dave Delta** ([coachdaveacademy.com/delta](https://coachdaveacademy.com/delta/)): Captures live in-sim telemetry and compares user brake and throttle traces against professional reference laps over distance or time. Its "Delta AI" auto-insights engine detects early/late braking points and trail-off truncation, but operates as post-session telemetry analysis rather than off-track training.
- **Garage 61** ([garage61.net](https://garage61.net/)): Community telemetry platform providing synchronized multi-lap overlays of brake, throttle, steering, and delta time. Identifies time gained or lost across corner phases without producing an isolated 0–100 drill score.
- **Trophi.ai** ([trophi.ai](https://www.trophi.ai/)): An AI coaching platform featuring Real-Time Skill Assessment (RTSA). Trophi.ai computes corner grades and provides auditory cues ("Brake Assist Tones") for braking markers. It tracks peak brake pressure, brake application timing, and release rate progression.

### 1.4 Open-source GitHub projects
- **TinyPedal** ([github.com/TinyPedal/TinyPedal](https://github.com/TinyPedal/TinyPedal)): Open-source Python overlay displaying pedal pressure bars, delta, and tire status in real time for rFactor 2 and Le Mans Ultimate.
- **irdashies** ([github.com/tariknz/irdashies](https://github.com/tariknz/irdashies)): Electron and TypeScript in-sim overlay suite for iRacing telemetry.
- **Jamesdavies1403/Sim-racing-app** (*Unverified* public availability): Early experimental Python repository analyzing `.ibt` files and implementing basic synthetic trace tracking.

Current landscape summary: No existing open-source tool provides a native, low-latency Windows executable combining 1 kHz DirectInput/SDL3 polling, isolated trace and hold drills, scientific motor scoring, and offline local leaderboards.

---

## 2. Motor-control science

In motor control research, continuous manual tracking tasks (both pursuit and compensatory tracking) assess how humans guide effectors to match dynamic reference signals ([Poulton, 1974](https://psycnet.apa.org/record/1975-04535-000)). Sim-racing pedal control is a continuous pursuit tracking task governed by visual feedforward perception and somatosensory (proprioceptive and kinesthetic) feedback.

### 2.1 Classical tracking-task error metrics

#### 2.1.1 Root Mean Square Error (RMSE)
Given target trajectory $y(t)$ and human trajectory $u(t)$ sampled at $N$ discrete points over duration $T$:

$$\text{RMSE} = \sqrt{\frac{1}{N} \sum_{i=1}^{N} (u(t_i) - y(t_i))^2} = \sqrt{\frac{1}{T} \int_{0}^{T} (u(t) - y(t))^2 \, dt}$$

- *Properties:* Operates in the $L_2$ norm. Squares deviations, disproportionately penalizing large excursions.
- *Sampling robustness:* Highly robust when normalized by $1/N$ (or integrated with $\Delta t$). Invariant to sampling rate changes above the signal Nyquist rate.
- *User interpretability:* Directly expressed in pedal units (e.g., "3.2% error"). Highly intuitive.

#### 2.1.2 Integrated Absolute Error (IAE) and Mean Absolute Error (MAE)

$$\text{IAE} = \int_{0}^{T} |u(t) - y(t)| \, dt, \quad \text{MAE} = \frac{1}{N} \sum_{i=1}^{N} |u(t_i) - y(t_i)|$$

- *Properties:* Operates in the $L_1$ norm. Linear penalty response. Robust against transient outlier spikes compared to RMSE.
- *User interpretability:* Expressed directly in pedal percentage units.

#### 2.1.3 Time on Target (ToT) / Time in Band (TiB)

$$\text{TiB} = \frac{1}{T} \int_{0}^{T} \mathbb{I}\left(|u(t) - y(t)| \le \Delta\right) \, dt \times 100\%$$

where $\Delta$ is the half-width of the tolerance band (e.g., $\Delta = 0.08$ for $\pm 8\%$), and $\mathbb{I}(\cdot)$ is the indicator function.
- *Properties:* Standard metric in psychomotor tracking literature ([Schmidt et al., 2018](https://us.humankinetics.com/products/motor-learning-and-performance-6th-edition-with-web-study-guide)). Evaluates functional task success.
- *Sampling robustness:* Highly robust and invariant to duration and sampling rate when discretized with $\Delta t = 1/f_s$.
- *User interpretability:* Excellent. Readily understood as a percentage (e.g., "88% time in band").
- *Limitation:* Insensitive to the magnitude of error outside the band (a 9% deviation is penalized identically to an 80% deviation). Must be paired with RMSE.

#### 2.1.4 Temporal lag via cross-correlation
Continuous pursuit tracking decomposes total error into spatial amplitude error and temporal phase lag ([Jagacinski & Flach, 2003](https://www.taylorfrancis.com/books/mono/10.4324/9781410607560/control-theory-humans-richard-jagacinski-john-flach)). Simple visual reaction time introduces a 150–250 ms delay in reactive tracking, whereas predictive feedforward tracking reduces lag to near zero or leads by 20–50 ms. Cross-correlation quantifies this phase offset independently of amplitude scaling.

### 2.2 Movement smoothness metrics
Smoothness reflects motor coordination, voluntary sub-movement blending, and sensorimotor efficiency ([Flash & Hogan, 1985](https://doi.org/10.1523/JNEUROSCI.05-07-01688.1985)). Unsmooth movements exhibit jerky sub-movements, corrections, and tremors.

#### 2.2.1 Cumulative squared jerk (unnormalized)
Jerk is the third time derivative of displacement (or second derivative of pedal percentage velocity): $j(t) = \frac{d^3 x}{dt^3}$.

$$J_{raw} = \int_{0}^{T} j(t)^2 \, dt$$

- *Critical flaw:* $J_{raw}$ strongly depends on movement duration $T$ and amplitude $D$. Scaling duration by $\alpha$ scales $J_{raw}$ by $\alpha^{-5}$; scaling amplitude by $\beta$ scales $J_{raw}$ by $\beta^2$ ([Hogan & Sternad, 2009](https://doi.org/10.3200/JMBR.41.6.529-534)). It cannot compare drills of differing lengths or amplitudes.

#### 2.2.2 Dimensionless Jerk (DJ) and Log Dimensionless Jerk (LDLJ)
To resolve dimensional scaling, [Hogan & Sternad (2009)](https://doi.org/10.3200/JMBR.41.6.529-534) and [Balasubramanian et al. (2015)](https://doi.org/10.1186/s12984-015-0090-9) established Dimensionless Jerk ($DJ$) and Log Dimensionless Jerk ($LDLJ$):

$$DJ = \frac{T^5}{D^2} \int_{0}^{T} j(t)^2 \, dt \quad \text{or} \quad DJ_v = \frac{T^3}{v_{peak}^2} \int_{0}^{T} j(t)^2 \, dt$$

$$LDLJ = -\ln(DJ)$$

- *Properties:* Invariant to temporal duration $T$ and movement scale $D$ (or peak velocity $v_{peak}$).
- *Theoretical benchmark:* For an idealized point-to-point minimum-jerk trajectory ([Flash & Hogan, 1985](https://doi.org/10.1523/JNEUROSCI.05-07-01688.1985)), $DJ \approx 360$, yielding $LDLJ \approx -\ln(360) \approx -5.886$.
- *Direction:* Values closer to 0 (less negative) denote smoother movements. Intermittent or jerky movements produce larger $DJ$ and more negative $LDLJ$ (e.g., $-8.5$ to $-12.0$).

#### 2.2.3 Spectral Arc Length (SPARC)
Introduced by [Balasubramanian et al. (2012, 2015)](https://doi.org/10.1186/s12984-015-0090-9), SPARC measures smoothness as the negative arc length of the normalized Fourier magnitude spectrum of the movement speed profile:

$$SPARC \triangleq -\int_{0}^{\omega_c} \sqrt{\left(\frac{1}{\omega_c}\right)^2 + \left(\frac{d|\hat{V}(\omega)|}{d\omega}\right)^2} \, d\omega$$

where $|\hat{V}(\omega)| = \frac{|V(\omega)|}{V(0)}$ is the DC-normalized Fourier magnitude spectrum of speed $v(t) = |\dot{u}(t)|$, and $\omega_c$ is an adaptive cutoff frequency bounded by $\omega_{max} = 20\text{ Hz}$ or where $|\hat{V}(\omega)|$ permanently drops below a threshold (typically 0.05).
- *Properties:* Monotonic, highly robust against sensor noise, and invariant to movement amplitude and duration ([Balasubramanian et al., 2015](https://doi.org/10.1186/s12984-015-0090-9)).
- *User interpretability:* SPARC values are negative real numbers (typically ranging from $-1.3$ for smooth trajectories to $-3.5$ for jerky trajectories). While mathematically superior, SPARC must be converted into a calibrated 0–100 scale to be understood by users.

### 2.3 Metric comparison matrix

| Metric | Robust to sample rate? | Robust to duration? | User interpretability | Implementation cost |
|---|---|---|---|---|
| **Time in Band (TiB)** | High (when discretized by $\Delta t$) | High (normalized to 100%) | Immediate (%) | Low ($O(N)$) |
| **RMSE** | High | High (mean squared) | Immediate (% pedal) | Low ($O(N)$) |
| **IAE / MAE** | High | High | Immediate (% pedal) | Low ($O(N)$) |
| **Lag ($\tau^*$)** | High (with sub-sample interp.) | High | Immediate (ms) | Low ($O(W \cdot N)$) |
| **Raw Jerk ($J_{raw}$)** | Low (noise amplifier) | None (scales as $T^{-5}$) | Unusable raw | Medium ($O(N)$) |
| **LDLJ** | Medium (requires filtering) | High (dimensionless) | Abstract (requires mapping) | Medium ($O(N)$) |
| **SPARC** | High | High | Abstract (requires mapping) | Medium ($O(N \log N)$ FFT) |

---

## 3. Lag estimation

Tracking performance conflates time delay (lag) with shape reproduction errors. A driver who replicates a target shape with a 100 ms constant reaction delay will register poor instantaneous RMSE despite displaying excellent pedal modulation. Decomposing these dimensions is essential for actionable coaching.

### 3.1 Cross-correlation implementation details

#### 3.1.1 Normalized cross-correlation
Let $y[n]$ be the target curve and $u[n]$ be the user pedal input sampled at $f_s = 1000\text{ Hz}$ for $n = 0, \dots, N-1$.
The zero-mean normalized cross-correlation (Pearson correlation coefficient at lag $k$) is defined as:

$$r[k] = \frac{\sum_{n} (u[n+k] - \bar{u}_k)(y[n] - \bar{y}_k)}{\sqrt{\sum_{n} (u[n+k] - \bar{u}_k)^2 \sum_{n} (y[n] - \bar{y}_k)^2}}$$

where $k \in [-K_{max}, +K_{max}]$ represents the discrete shift.
- *Search Range:* Set search window to $\pm 300\text{ ms}$ ($K_{max} = 300$ samples at 1 kHz). Simple human reaction time is approximately 180–250 ms, while anticipatory lead rarely exceeds 100 ms. A window of $\pm 300\text{ ms}$ captures realistic motor timing without producing spurious cycle matches.

#### 3.1.2 Sub-sample peak interpolation
Discrete sampling at 1 kHz yields 1 ms resolution. To obtain sub-millisecond precision and prevent quantization stepping in timing scores, apply parabolic (quadratic) interpolation across the discrete correlation peak ([de Cheveigné & Kawahara, 2002](https://doi.org/10.1121/1.1458024)):
1. Find discrete integer index maximizing correlation: $m = \arg\max_{k} r[k]$.
2. Extract the three values: $\alpha = r[m-1]$, $\beta = r[m]$, and $\gamma = r[m+1]$.
3. Compute the fractional sample offset $p \in [-0.5, +0.5]$:

$$p = \frac{1}{2} \cdot \frac{\alpha - \gamma}{\alpha - 2\beta + \gamma}$$

4. The estimated lag in seconds is:

$$\tau^* = (m + p) \cdot \Delta t = \frac{m + p}{1000}$$

#### 3.1.3 Handling flat segments and low-variance signals
If a segment has zero variance (e.g., target or user holds a constant pedal position or rests at 0%), the denominator of Pearson's $r[k]$ evaluates to zero.
- *Rule 1 (Variance Gate):* Compute signal standard deviations $\sigma_y$ and $\sigma_u$. If $\sigma_y < 0.01$ (less than 1% variation across the window), cross-correlation is ill-conditioned. Return $\tau^* = 0\text{ ms}$ and bypass correlation lag estimation.
- *Rule 2 (Dynamic Windowing):* In drills with long lead-in rests (0% pedal) or extended post-drill holds, compute cross-correlation only over the *active dynamic window* where $|dy/dt| > 0.05\text{ s}^{-1}$ or $y(t) > 0.02$.

### 3.2 Scoring after lag compensation vs. before
- *The double-penalty problem:* If raw uncompensated RMSE is scored, any timing delay $\tau$ inflates RMSE: $\text{RMSE} \approx \tau \cdot \sqrt{\text{mean}(\dot{y}^2)}$. If the timing sub-score simultaneously docks points for lag $\tau$, the driver is penalized twice for a single motor error.
- *Decoupled scoring architecture:*
  1. Estimate lag $\tau^*$ using cross-correlation.
  2. Shift the user trace by $-\tau^*$ (or sample target at $t + \tau^*$) to construct the *lag-aligned user trace* $u_{aligned}(t) = u(t + \tau^*)$.
  3. Compute **Shape Accuracy** (lag-compensated RMSE and lag-compensated TiB) using $u_{aligned}(t)$ against $y(t)$.
  4. Compute **Timing Score** independently using $\tau^*$.
This separates shape control (pedal pressure modulation) from temporal synchronization (reaction timing).

### 3.3 Asymmetric timing penalty: anticipation vs. reaction delay
In automotive dynamics, braking late is significantly more hazardous than braking slightly early:
- *Braking late ($\tau^* > 0$):* Causes missed corner turn-in points, front tire overload, corner runoff, or vehicle collisions.
- *Braking early ($\tau^* < 0$):* Represents anticipatory feedforward behavior. While slightly sub-optimal for minimum lap time, vehicle stability and corner entry control are fully preserved.

*Timing penalty rule:* Penalize late reactions ($\tau^* > 0$) more heavily than anticipatory leads ($\tau^* < 0$).
- Allow an anticipatory grace band of $[-30\text{ ms}, 0\text{ ms}]$ with zero penalty.
- Penalize late lag starting at $\tau^* > +15\text{ ms}$.
- Scale late penalties at $1.5\times$ to $2.0\times$ the rate of early lead penalties.

### 3.4 Dynamic Time Warping (DTW) and its risks
Dynamic Time Warping aligns two time series by finding an optimal non-linear warping path that minimizes Euclidean distance ([Sakoe & Chiba, 1978](https://doi.org/10.1109/TASSP.1978.1163109)).
- *When appropriate:* Speech phoneme recognition, handwriting recognition, and gesture classification where execution speed varies naturally and temporal duration is irrelevant.
- *Why DTW masks timing errors in sim racing:* In racing drills, temporal execution is the primary skill. DTW warps time non-linearly, compressing and stretching arbitrary segments. If a driver hesitates for 200 ms mid-trail or releases pressure at half the required rate, DTW collapses the temporal mismatch along the warping diagonal and reports high shape similarity. This conceals the timing error. Standard linear cross-correlation lag estimation must be used instead of DTW.

---

## 4. Smoothness

### 4.1 Computing jerk from 1 kHz noisy pedal data
Calculating jerk requires taking the third numerical derivative of position (or second derivative of pedal percentage velocity):

$$j(t) = \frac{d^3 u(t)}{dt^3}$$

In the frequency domain, numerical differentiation acts as a high-pass filter with frequency response $H(j\omega) = (j\omega)^3$. High frequencies are amplified proportional to $\omega^3$. At a 1 kHz sampling rate, 12-bit ADC quantization stepping and load-cell amplifier thermal noise completely overwhelm the signal if differentiated raw.

```
Raw pedal (1 kHz) ──► [Low-pass / Savitzky-Golay (10–12 Hz)] ──► Numerical differentiation ──► Jerk j(t)
```

#### 4.1.1 Biomechanical cutoff frequency
Voluntary human limb motor control is bandwidth-limited:
- Voluntary motor adjustments occur between 1 Hz and 5 Hz.
- Rapid ballistic corrections peak around 8–10 Hz ([Wickens & Hollands, 2000](https://www.pearson.com/en-us/subject-catalog/p/engineering-psychology-and-human-performance/P200000003554)).
- Physiological tremor operates between 8 Hz and 12 Hz.
- Frequencies above 15 Hz consist entirely of electronic noise, pedal mechanical resonance, and ADC quantization.

*Cutoff selection:* A low-pass cutoff frequency of $f_c = 10\text{–}12\text{ Hz}$ preserves human motor transients while attenuating differentiation-amplified noise.

#### 4.1.2 Filter implementation options
1. **Zero-phase Butterworth filter:** A 4th-order low-pass Butterworth filter applied bidirectionally (`filtfilt`) with cutoff $f_c = 12\text{ Hz}$. Zero-phase filtering eliminates phase lag, ensuring peak alignment is preserved.
2. **Savitzky-Golay polynomial filter:** Fits a local polynomial of degree $p$ over a moving window of $2m+1$ samples ([Savitzky & Golay, 1964](https://doi.org/10.1021/ac60214a047)). It outputs smoothed derivatives in a single convolution step:
   - For $f_s = 1000\text{ Hz}$, select polynomial order $p = 3$ and window length $W = 71\text{ ms}$ (71 samples). This acts as a low-pass differentiator with effective cutoff around 11 Hz, effectively smoothing ADC quantization steps.

### 4.2 Trail-braking release jerk
In race driving, the brake pedal release phase (trail braking) dictates vehicle pitch attitude, front-to-rear load transfer, and front tire grip during corner entry.
- *Physical consequence:* Abrupt or erratic brake release decompresses the front suspension rapidly. This unloads the front tires, inducing understeer, or snaps the rear axle into oversteer.
- *Definition of release phase:* The temporal interval $[t_{peak}, t_{end}]$, where $t_{peak}$ is the instant of maximum brake pressure ($u(t_{peak}) = \max u(t)$), and $t_{end}$ is the point where brake pressure drops below 1% ($u(t) \le 0.01$).
- *Release jerk metrics:*
  1. *Root Mean Square Release Jerk:* $\text{RMSJ}_{rel} = \sqrt{\frac{1}{T_{rel}} \int_{t_{peak}}^{t_{end}} j(t)^2 \, dt}$.
  2. *Inflection count (stuttering):* Count sign reversals of the first derivative $\dot{u}(t)$ during release (where $\dot{u}(t) > 0$). A proper trail brake is monotonically decreasing ($\dot{u}(t) \le 0$); positive spikes represent hesitation or foot trembling.
  3. *Normalized Release Jerk:* Apply dimensionless jerk over the release interval $[t_{peak}, t_{end}]$ to prevent long, gradual trail phases from scoring artificially worse than rapid stabs.

### 4.3 Peak overshoot
Peak overshoot measures the error between the maximum user input and the target maximum:

$$E_{os} = \max_{t} u(t) - \max_{t} y(t)$$

- In GT3 and non-ABS sim racing, exceeding target peak pressure locks the tires or triggers aggressive ABS intervention, extending braking distance.
- Overshoot should be evaluated as an asymmetric penalty applied to the smoothness/control sub-score: undershoot is evaluated as an accuracy tracking error, whereas overshoot ($E_{os} > 0$) receives an explicit penalty.

---

## 5. Combining scores

### 5.1 Mapping raw metrics to 0–100 sub-scores
Raw engineering metrics (RMSE in %, lag in ms, jerk in $\text{s}^{-3}$) must be transformed into bounded $[0, 100]$ sub-scores.

```
       100 ┌─────────\
           │          \   Exponential / Logistic
           │           \
    Score  │            \
           │             \───
         0 └──────────────────────
           0     Error Metric (x)
```

#### 5.1.1 Comparison of mapping functions
- **Linear ramp:** $S(x) = \max(0, 100 \cdot (1 - x / x_{max}))$.
  - *Disadvantages:* Creates a rigid cliff at $x_{max}$ with zero gradient for larger errors. Fails to reward improvements beyond $x_{max}$.
- **Exponential decay:** $S(x) = 100 \cdot \exp\left(-x / x_0\right)$.
  - *Advantages:* Asymptotically approaches zero, provides smooth non-zero gradients everywhere, and models psychophysical perception ([Weber-Fechner law](https://en.wikipedia.org/wiki/Weber%E2%80%93Fechner_law)). The parameter $x_0 = x_{50} / \ln(2)$ represents the characteristic decay constant.
- **Logistic / Sigmoidal with deadband:**

$$S(x) = \frac{100}{1 + \left(\frac{\max(0, x - x_{dead})}{x_{50}}\right)^p}$$

  - *Advantages:* Provides a forgiving "elite deadband" ($x \le x_{dead}$ yields 100), a steep middle discrimination zone, and a gentle floor. Ideal for motor skills where sub-millimeter physiological noise should not be penalized.

### 5.2 Sub-score weighting and orthogonality
To prevent double-penalizing single errors:
- **Accuracy Sub-Score ($S_{acc}$):** Computed on **lag-compensated** data. Combines Time in Band ($S_{tib}$, weight 0.50) and lag-compensated RMSE ($S_{rmse}$, weight 0.50).
- **Timing Sub-Score ($S_{time}$):** Evaluates absolute cross-correlation lag $|\tau^*|$ with directional asymmetry.
- **Smoothness Sub-Score ($S_{smooth}$):** Evaluates release phase Log Dimensionless Jerk ($S_{ldlj}$, weight 0.70) and peak overshoot penalty ($S_{os}$, weight 0.30).

#### Recommended trace drill weights:

$$S_{total} = 0.50 \cdot S_{acc} + 0.25 \cdot S_{time} + 0.25 \cdot S_{smooth}$$

#### Recommended hold drill weights (SCT-032):

$$S_{total,hold} = 0.60 \cdot S_{acc} + 0.20 \cdot S_{time} + 0.20 \cdot S_{smooth}$$

where for hold drills:
- $S_{acc}$: Time in Band (0.60) + steady-state RMSE (0.40).
- $S_{time}$: Time to enter the tolerance band ($t_{settle}$).
- $S_{smooth}$: Peak overshoot during initial attack ($E_{os}$) + steady-state tremor/jitter ($S_{jitter}$).

### 5.3 Grade scale thresholds
Aligned with decision log D-09 and backlog SCT-032:
- **S-Grade:** $\ge 95.0$ (Near-perfect professional execution)
- **A-Grade:** $\ge 85.0$ (Clean, highly controlled)
- **B-Grade:** $\ge 70.0$ (Solid club-level execution; minor timing or wobble)
- **C-Grade:** $\ge 55.0$ (Noticeable hesitation, lag, or band excursions)
- **D-Grade:** $< 55.0$ (Incomplete, delayed, or uncontrolled trace)

### 5.4 Repetition consistency (SCT-033)
Motor learning requires reducing trial-to-trial variance ([Schmidt et al., 2018](https://us.humankinetics.com/products/motor-learning-and-performance-6th-edition-with-web-study-guide)). A drill set consists of $N$ repetitions (default $N = 5$).

#### 5.4.1 Score variance vs. trace variance
Consistency can be computed in two ways:
1. *Score-level Standard Deviation:* Sample standard deviation of total rep scores $S_1, \dots, S_N$:

$$\sigma_S = \sqrt{\frac{1}{N-1} \sum_{i=1}^{N} (S_i - \bar{S})^2}$$

2. *Point-wise Trajectory Variance (Trace CV):* Time-averaged standard deviation across the $N$ aligned pedal traces:

$$\bar{\sigma}_{trace} = \frac{1}{T} \int_{0}^{T} \sqrt{\frac{1}{N-1} \sum_{i=1}^{N} (u_i(t) - \bar{u}(t))^2} \, dt$$

#### 5.4.2 Consistency sub-score formula
Using score standard deviation $\sigma_S$, define consistency score $S_{cons} \in [0, 100]$:

$$S_{cons} = 100 \cdot \exp\left(-\frac{\sigma_S}{7.5}\right)$$

- If $\sigma_S = 0.0$ (identical scores), $S_{cons} = 100$.
- If $\sigma_S = 2.0$ points, $S_{cons} \approx 76.6$.
- If $\sigma_S = 5.0$ points, $S_{cons} \approx 51.3$.
- If $\sigma_S \ge 10.0$ points, $S_{cons} \le 26.3$.

---

## 6. Tolerance band and difficulty

### 6.1 The tolerance band debate: ±8% vs. ±4% (Q-03)
In `docs/open-questions.md`, Q-03 asks whether the default tolerance band should be a forgiving $\pm 8\%$ (tightening with level) or a strict $\pm 4\%$ from the start.
- Braking Lab exercise examples on its site use ±8% (e.g. "Threshold 90% - Trail Release ... 3 s · ±8%"). A configurable range is **Unverified**.
- In sim-racing load-cell pedals, human foot tremor and pedal flex produce $\pm 1\text{–}2\%$ natural resting variance even during steady holding.

### 6.2 Motor learning literature principles

#### 6.2.1 Challenge Point Framework
[Guadagnoli & Lee (2004)](https://doi.org/10.3200/JMBR.36.2.212-224) established that learning is optimized when the task's functional difficulty matches the performer's skill level.
- If task difficulty is too high (e.g., $\pm 4\%$ band for a novice), information overload occurs: the performer cannot interpret error feedback, causing frustration and erratic motor search strategies.
- If task difficulty is too low (e.g., $\pm 15\%$ for an experienced driver), the task presents zero learning challenge, and no motor adaptation occurs.

#### 6.2.2 Knowledge of Results (KR) vs. Knowledge of Performance (KP)
- *Knowledge of Results (KR):* Post-trial outcome score and letter grade (e.g., "88/100, Grade A").
- *Knowledge of Performance (KP):* Real-time kinematic feedback (the scrolling graph, the green/red tolerance rails, and live audio error tones).

#### 6.2.3 Bandwidth feedback and the Guidance Hypothesis
[Salmoni, Schmidt, & Walter (1984)](https://doi.org/10.1037/0033-2909.95.3.355) formulated the *Guidance Hypothesis*: while continuous real-time feedback enhances immediate trial performance, providing it constantly creates sensory dependency. The learner relies on external visual/auditory guidance rather than developing intrinsic proprioceptive error detection.

[Sherwood (1988)](https://doi.org/10.2466/pms.1988.66.2.535) and [Lee & Carnahan (1990)](https://doi.org/10.1080/14640749008401248) proved that **bandwidth feedback** resolves this dilemma:
- Feedback is provided *only* when error exceeds the tolerance band.
- Inside the tolerance band, feedback is silent (or displays green neutral status).
- This creates an intrinsic "faded feedback" schedule ([Winstein & Schmidt, 1990](https://doi.org/10.1037/0278-7393.16.4.677)): as motor competence grows, the performer spends more time inside the band, naturally reducing external guidance and solidifying permanent muscle memory.

### 6.3 Difficulty progression recommendation
SimCurveTrainApp should implement a multi-tiered difficulty band:
1. **Rookie / Casual:** $\pm 8\%$ default band. Forgiving on transient twitches; allows new users to learn basic curve geometry and timing without immediate failure.
2. **Clubman / Standard:** $\pm 6\%$ band. Unlocked at License Tier C/B.
3. **Pro / Alien:** $\pm 4\%$ band. Unlocked at License Tier A/Pro. Demands professional load-cell modulation.

---

## 7. Concrete recommendations

### 7.1 Trace scoring mathematical specifications

#### Pre-processing pipeline (executed at drill completion in Rust core):
1. **Input signals:** Target $y[n]$ and User $u[n]$ sampled at $f_s = 1000\text{ Hz}$ for $N$ samples ($n = 0, \dots, N-1$).
2. **Signal conditioning:**
   - Apply a 4th-order bidirectional zero-phase low-pass Butterworth filter ($f_c = 12\text{ Hz}$) to $u[n]$ to produce filtered pedal position $u_f[n]$.
   - Compute discrete velocity $v_f[n] = (u_f[n+1] - u_f[n-1]) \cdot \frac{f_s}{2}$.
   - Compute discrete acceleration $a_f[n] = (v_f[n+1] - v_f[n-1]) \cdot \frac{f_s}{2}$.
   - Compute discrete jerk $j_f[n] = (a_f[n+1] - a_f[n-1]) \cdot \frac{f_s}{2}$.

#### Step 1: Lag estimation ($\tau^*$)
- Search range: $k \in [-300, +300]$ samples ($\pm 300\text{ ms}$).
- Verify dynamic range: if $\text{std}(y) < 0.01$, set $\tau^* = 0.0\text{ ms}$.
- Otherwise, compute zero-mean Pearson cross-correlation $r[k]$ over the active drill segment.
- Locate discrete peak index $m = \arg\max_k r[k]$.
- Apply parabolic interpolation with neighbors $\alpha = r[m-1], \beta = r[m], \gamma = r[m+1]$:

$$p = \frac{\alpha - \gamma}{2(\alpha - 2\beta + \gamma)}, \quad \tau^* = \frac{m + p}{1000} \text{ (seconds)}$$

#### Step 2: Timing sub-score ($S_{time}$)
Map lag $\tau^*$ (in milliseconds, $\tau_{ms} = 1000 \cdot \tau^*$) using asymmetric logistic decay:

$$\tau_{eff} = \begin{cases}
0 & \text{if } -30 \le \tau_{ms} \le 10 \\
\tau_{ms} - 10 & \text{if } \tau_{ms} > 10 \text{ (late)} \\
|\tau_{ms}| - 30 & \text{if } \tau_{ms} < -30 \text{ (early lead)}
\end{cases}$$

$$S_{time} = \frac{100}{1 + \left(\frac{\tau_{eff}}{\tau_{half}}\right)^2}$$

where $\tau_{half} = 80\text{ ms}$ for late lag, and $\tau_{half} = 140\text{ ms}$ for early lead.
- 0 to 10 ms lag $\rightarrow S_{time} = 100$.
- 50 ms late $\rightarrow \tau_{eff} = 40\text{ ms} \rightarrow S_{time} = 80.0$.
- 100 ms late $\rightarrow \tau_{eff} = 90\text{ ms} \rightarrow S_{time} = 44.1$.

#### Step 3: Lag-compensated shape accuracy sub-score ($S_{acc}$)
Construct lag-aligned trace $u_{align}[n]$ by evaluating $u_f$ shifted by $\tau^*$:

$$e_{align}[n] = u_f\left[n + \text{round}(1000\tau^*)\right] - y[n]$$

1. **Time in Band ($S_{tib}$):**

$$\text{TiB} = \frac{1}{N} \sum_{n=0}^{N-1} \mathbb{I}\left(|e_{align}[n]| \le \Delta_{band}\right) \times 100$$

2. **Lag-compensated RMSE ($S_{rmse}$):**

$$\text{RMSE}_{align} = \sqrt{\frac{1}{N} \sum_{n=0}^{N-1} (e_{align}[n])^2}$$

$$S_{rmse} = 100 \cdot \exp\left(-\frac{\text{RMSE}_{align}}{0.06}\right)$$

(where $\text{RMSE} = 0.02 \rightarrow 71.6$, $\text{RMSE} = 0.04 \rightarrow 51.3$).
3. **Combined Accuracy:**

$$S_{acc} = 0.50 \cdot \text{TiB} + 0.50 \cdot S_{rmse}$$

#### Step 4: Smoothness sub-score ($S_{smooth}$)
1. **Identify trail-brake release segment:** Locate index of peak target pressure $n_{peak} = \arg\max y[n]$. Define release window from $n_{peak}$ until $y[n] \le 0.01$ (duration $T_{rel}$, peak release velocity $v_{max,rel}$).
2. **Log Dimensionless Jerk ($LDLJ_{rel}$):**

$$DJ_{rel} = \frac{T_{rel}^3}{v_{max,rel}^2} \sum_{n=n_{peak}}^{n_{end}} (j_f[n])^2 \cdot \Delta t$$

$$LDLJ_{rel} = -\ln(\max(DJ_{rel}, 360.0))$$

$$S_{jerk} = \max\left(0, \min\left(100, 100 \cdot \left(1 - \frac{|LDLJ_{rel}| - 5.88}{6.0}\right)\right)\right)$$

3. **Peak Overshoot Penalty ($S_{os}$):**

$$E_{os} = \max\left(0, \max(u_f) - \max(y)\right)$$

$$S_{os} = 100 \cdot \exp\left(-\frac{E_{os}}{0.05}\right)$$

4. **Combined Smoothness:**

$$S_{smooth} = 0.70 \cdot S_{jerk} + 0.30 \cdot S_{os}$$

#### Step 5: Total trace score ($S_{total}$)

$$S_{total} = 0.50 \cdot S_{acc} + 0.25 \cdot S_{time} + 0.25 \cdot S_{smooth}$$

---

### 7.2 Hold scoring mathematical specifications (SCT-032)
For a constant hold drill at target level $Y_{target}$ (e.g., 70% brake) over hold window $T_{hold}$ with tolerance band $\Delta_{band}$:

1. **Settling time ($t_{settle}$):** Elapsed time from drill start until $u_f(t)$ enters and remains inside $[Y_{target} - \Delta_{band}, Y_{target} + \Delta_{band}]$ for at least 200 ms.

$$S_{time,hold} = \max\left(0, \min\left(100, 100 \cdot \left(1 - \frac{\max(0, t_{settle} - 0.20)}{1.0}\right)\right)\right)$$

2. **Hold accuracy ($S_{acc,hold}$):** Evaluated over the hold window $[t_{settle}, T_{end}]$:

$$\text{TiB}_{hold} = \frac{T_{\text{inside}}}{T_{window}} \times 100$$

$$\text{RMSE}_{hold} = \sqrt{\frac{1}{M} \sum_{m} (u_f[m] - Y_{target})^2}$$

$$S_{rmse,hold} = 100 \cdot \exp\left(-\frac{\text{RMSE}_{hold}}{0.03}\right)$$

$$S_{acc,hold} = 0.60 \cdot \text{TiB}_{hold} + 0.40 \cdot S_{rmse,hold}$$

3. **Hold stability / smoothness ($S_{smooth,hold}$):**
   - Peak overshoot: $E_{os} = \max(0, \max(u_f) - Y_{target})$.
   - Steady-state jitter (RMS velocity of filtered input during hold):

$$\text{Jitter} = \sqrt{\frac{1}{M}\sum_m (v_f[m])^2}$$

$$S_{smooth,hold} = 0.50 \cdot 100 e^{-E_{os}/0.04} + 0.50 \cdot 100 e^{-\text{Jitter}/0.08}$$

4. **Total hold score:**

$$S_{total,hold} = 0.60 \cdot S_{acc,hold} + 0.20 \cdot S_{time,hold} + 0.20 \cdot S_{smooth,hold}$$

---

### 7.3 Synthetic test cases and expected score ordering

To satisfy the acceptance criteria of SCT-036, test the algorithm against synthetic traces generated against a standard GT3 hairpin brake curve ($T = 2.5\text{ s}$, peak 90%, 1.8 s trail release, $\Delta_{band} = \pm 8\%$):

| Test Case | Description | Expected $S_{acc}$ | Expected $S_{time}$ | Expected $S_{smooth}$ | Expected $S_{total}$ | Expected Grade |
|---|---|---|---|---|---|---|
| **Case 1: Exact copy** | $u(t) \equiv y(t)$. Perfect tracking. | 100 | 100 | 100 | **99–100** | **S** |
| **Case 2: Minor motor jitter** | Within tolerance band ($\pm 2\%$), smooth, $\tau = +8\text{ ms}$. | 96 | 100 | 90 | **95–97** | **S** |
| **Case 3: 100 ms pure delay** | Identical shape shifted by $+100\text{ ms}$. Perfect modulation, late reaction. | 98 (aligned) | 44 | 100 | **85–86** | **A** |
| **Case 4: Noisy trace** | Centered on target, but contains high-frequency tremor and jagged trail steps. | 88 | 100 | 48 | **77–81** | **B** |
| **Case 5: Peak overshoot** | Reaches 105% (+15% overshoot), stuttered 2-step trail release. | 70 | 95 | 38 | **68–72** | **B / C** |
| **Case 6: Delayed + Overshoot** | 120 ms late, +12% overshoot, jerky release. | 65 | 32 | 35 | **49–55** | **C / D** |
| **Case 7: Off-target** | Constant 40% offset or wild miss. | 10 | 0 | 50 | **< 20** | **D** |

*Verification of acceptance ordering:*
$$\text{Exact copy (S)} > \text{Minor jitter (S)} > \text{100 ms delay (A)} > \text{Noisy (B)} > \text{Overshoot (B/C)} > \text{Delayed + Overshoot (C/D)} > \text{Gross miss (D)}$$

---

### 7.4 Implementation pitfalls
1. **Direct Differentiation of Unfiltered 1 kHz Data:**
   - *Problem:* Applying finite differences directly to 1 kHz ADC values causes numerical explosion in jerk ($\Delta t^3 = 10^{-9}$).
   - *Fix:* Always filter with a zero-phase low-pass Butterworth ($f_c = 12\text{ Hz}$) or Savitzky-Golay filter prior to differentiation.
2. **Zero-Division in Flat Signals:**
   - *Problem:* Calculating Pearson cross-correlation on flat hold signals or lead-in zero buffers divides by zero standard deviation.
   - *Fix:* Detect signals where $\text{std}(y) < 0.01$ or $\text{std}(u) < 0.01$, bypass cross-correlation, and set lag to zero.
3. **Filter Boundary (Edge) Artifacts:**
   - *Problem:* Bidirectional IIR filters and Savitzky-Golay convolutions produce transient edge oscillations at $t = 0$ and $t = T$.
   - *Fix:* Pad the beginning and end of the sample buffer using mirror padding or constant value extension by at least $3 \times$ the filter window size before filtering.
4. **Double-Penalizing Latency:**
   - *Problem:* Calculating RMSE on unaligned traces heavily penalizes lag twice (in RMSE and in the timing sub-score).
   - *Fix:* Calculate accuracy metrics on the lag-compensated trace $u(t + \tau^*)$.
5. **Feedback Dependency:**
   - *Problem:* Emitting continuous real-time error tones creates audio guidance dependency, hindering long-term retention.
   - *Fix:* Adhere strictly to bandwidth feedback principles: remain silent while inside the tolerance band; sound tones only when the user crosses outside the band.

---

### 7.5 Open questions for the maintainer

1. **Dynamic vs. Static Tolerance Bands:**
   Should the tolerance band width $\Delta_{band}$ remain constant throughout a curve, or vary dynamically (e.g., wider $\pm 10\%$ during the initial high-rate brake strike, narrowing to $\pm 4\%$ during delicate trail-off)? Real-world telemetry displays far greater variance during the attack phase than the release phase.
2. **Audio Feedback Modality:**
   In trace drills, should the audio tone provide continuous signed pitch indicating instantaneous direction of error (higher pitch = too much brake), or should it remain completely silent within the tolerance band and sound only when breaching the boundary (true bandwidth feedback)?
3. **Hardware Normalization:**
   Different load-cell hardware exhibits varied stiffness (e.g., 20 kg soft rubber vs. 100 kg hydraulic-feel elastomer). Should consistency and jitter scoring incorporate hardware-specific thresholds calibrated during SCT-014?

---

## Sources

- Balasubramanian, S., Melendez-Calderon, A., & Burdet, E. (2012). A robust and sensitive metric for quantifying movement smoothness. *IEEE Transactions on Biomedical Engineering*, 59(8), 2126–2136. [doi:10.1109/TBME.2012.2200890](https://doi.org/10.1109/TBME.2012.2200890)
- Balasubramanian, S., Melendez-Calderon, A., Roby-Brami, A., & Burdet, E. (2015). On the analysis of movement smoothness. *Journal of NeuroEngineering and Rehabilitation*, 12(1), 112. [doi:10.1186/s12984-015-0090-9](https://doi.org/10.1186/s12984-015-0090-9)
- Braking Lab. Official Documentation and Platform Changelog. [brakinglab.com](https://www.brakinglab.com/en)
- Coach Dave Academy. Delta Telemetry Analysis & Auto Insights. [coachdaveacademy.com/delta](https://coachdaveacademy.com/delta/)
- de Cheveigné, A., & Kawahara, H. (2002). YIN, a fundamental frequency estimator for speech and music. *The Journal of the Acoustical Society of America*, 111(4), 1917–1930. [doi:10.1121/1.1458024](https://doi.org/10.1121/1.1458024)
- Flash, T., & Hogan, N. (1985). The coordination of arm movements: an experimentally confirmed mathematical model. *The Journal of Neuroscience*, 5(7), 1688–1703. [doi:10.1523/JNEUROSCI.05-07-01688.1985](https://doi.org/10.1523/JNEUROSCI.05-07-01688.1985)
- Garage 61. Sim Racing Telemetry Platform. [garage61.net](https://garage61.net/)
- Guadagnoli, M. A., & Lee, T. D. (2004). Challenge point: a framework for conceptualizing the effects of various practice conditions in motor learning. *Journal of Motor Behavior*, 36(2), 212–224. [doi:10.3200/JMBR.36.2.212-224](https://doi.org/10.3200/JMBR.36.2.212-224)
- Hogan, N., & Sternad, D. (2009). Sensitivity of smoothness measures to movement duration, amplitude, and arrests. *Journal of Motor Behavior*, 41(6), 529–534. [doi:10.3200/JMBR.41.6.529-534](https://doi.org/10.3200/JMBR.41.6.529-534)
- HSimRacing. Brake Curve Analyzer. [hsimracing.com/brake-curve-analyzer](https://hsimracing.com/brake-curve-analyzer/)
- Jagacinski, R. J., & Flach, J. M. (2003). *Control Theory for Humans: Quantitative Approaches to Modeling Performance*. CRC Press. [taylorfrancis.com](https://www.taylorfrancis.com/books/mono/10.4324/9781410607560/control-theory-humans-richard-jagacinski-john-flach)
- Lee, T. D., & Carnahan, H. (1990). Bandwidth knowledge of results and motor learning: More than just a relative frequency effect. *The Quarterly Journal of Experimental Psychology*, 42(4), 777–789. [doi:10.1080/14640749008401248](https://doi.org/10.1080/14640749008401248)
- Poulton, E. C. (1974). *Tracking Skill and Manual Control*. Academic Press. [psycnet.apa.org](https://psycnet.apa.org/record/1975-04535-000)
- Sakoe, H., & Chiba, S. (1978). Dynamic programming algorithm optimization for spoken word recognition. *IEEE Transactions on Acoustics, Speech, and Signal Processing*, 26(1), 43–49. [doi:10.1109/TASSP.1978.1163109](https://doi.org/10.1109/TASSP.1978.1163109)
- Salmoni, A. W., Schmidt, R. A., & Walter, C. B. (1984). Knowledge of results and motor learning: evaluating the guidance hypothesis. *Psychological Bulletin*, 95(3), 355–386. [doi:10.1037/0033-2909.95.3.355](https://doi.org/10.1037/0033-2909.95.3.355)
- Savitzky, A., & Golay, M. J. (1964). Smoothing and differentiation of data by simplified least squares procedures. *Analytical Chemistry*, 36(8), 1627–1639. [doi:10.1021/ac60214a047](https://doi.org/10.1021/ac60214a047)
- Schmidt, R. A., Lee, T. D., Winstein, C., Wulf, G., & Zelaznik, H. N. (2018). *Motor Learning and Performance: From Principles to Application* (6th ed.). Human Kinetics. [humankinetics.com](https://us.humankinetics.com/products/motor-learning-and-performance-6th-edition-with-web-study-guide)
- Sherwood, D. E. (1988). Effect of bandwidth knowledge of results on movement consistency. *Perceptual and Motor Skills*, 66(2), 535–542. [doi:10.2466/pms.1988.66.2.535](https://doi.org/10.2466/pms.1988.66.2.535)
- Simucube. ActivePedal Technical Specification and Tuner Software. [simucube.com/activepedal](https://simucube.com/activepedal/)
- TinyPedal. Real-time Telemetry Overlay for Racing Simulations. [github.com/TinyPedal/TinyPedal](https://github.com/TinyPedal/TinyPedal)
- Trail Braking Trainer. Browser-based Trail Braking Tool. [trailbraking.uk](https://trailbraking.uk/)
- Trophi.ai. AI Sim Racing Coach & Real-Time Skill Assessment. [trophi.ai](https://www.trophi.ai/)
- Wickens, C. D., & Hollands, J. G. (2000). *Engineering Psychology and Human Performance* (3rd ed.). Prentice Hall. [pearson.com](https://www.pearson.com/en-us/subject-catalog/p/engineering-psychology-and-human-performance/P200000003554)
- Winstein, C. J., & Schmidt, R. A. (1990). Reduced frequency of knowledge of results enhances motor skill learning. *Journal of Experimental Psychology: Learning, Memory, and Cognition*, 16(4), 677–691. [doi:10.1037/0278-7393.16.4.677](https://doi.org/10.1037/0278-7393.16.4.677)
