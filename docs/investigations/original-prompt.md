I'd like to create a windows application for simracers to train/prepare pedal muscle memory.
The idea is to connect brake/throttle pedal, app will tell what pressure user should git once or for some time (maybe even changing graph), user tries that, app shows how accurate their input was.
Probably, the main audience is iRacing (at least for me).

The investigation at first should focus on searching if there is already similar software.
Then analyze how it could be achieved, I don't have a preference for now that language and other things should be used.
The analysis should be thorough, questions should be asked about what exactly I want an dhow it should look like.

Features and things I'd like to see (MVP could have basic limited functionality):
- it will be available on my public github with releases as .exe files so it's easy for users to install, no manual builds
- selecting/creating different presets of cars/car types (road/gt3/lmp/formula/nascar/etc.) which will change the goal of how pedal input should look like
- both options for constant pressure and expected pedal trace for a few secs should be available. It should be similar to the real braking/throttle applications from telemetry
- presets for specific car/car type could be created by ai agent/script by looking into best results in garage61 from best results and highest iRating publicly available telemetry.
This feature wouldn't be user facing, just manual script run/asking agent during development
- The interface should be easy to use, modern looking, fast (<50ms) without having noticable delay between real input and showing the graph (I have VNM pedals and their software has noticable dealy when trying to see my pedal inputs)
- pedal trace should show both current input bar 0-100% and a graph for latest few-5-10 secons (adjustable)
- leaderboard for your own results

Questionable features that could be skipped (might be too complex or other reasons, definetely not MVP):
- leaderboard of other users
- not only basic goals for pedal inputs to warmup before race but complete "career" mode where complete novice in simracing or experienced user could learn or improve their skill by time, not just after a few minutes/times
- sharing/importing other presets
- combining also a wheel to train smth like trail braking with correct wheel angle. The more brake force, the less wheel angle and vice versa
