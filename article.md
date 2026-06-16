Windows enthusiasts often look for ways to extract as much performance out of their systems as possible, and it's often the case that they try and do so while trying to minimize the heat and power consumption. This is especially relevant in the case of mobile Windows PCs since laptops and notebooks tend to get hot and management of that heat and power is harder in such a form factor.

As such users often turn to techniques like under-volting which can be used to squeeze out the maximum capabilities of a chip while also maintaining lowered power levels. There are official apps from AMD and Intel with the likes of Ryzen Master and XTU (Extreme Tuning Utility). While these are quite handy, most enthusiasts probably prefer to dig into the BIOS and play around with settings there like Curve Optimizer on Ryzen, which lets users set various frequency-voltage scaling values. These are essentially called P-States.

If you are not familiar with them, Processor Power Management is done through Advanced Configuration and Power Interface (ACPI) P-states and C-states. While P-states or performance pwoer states handle CPU voltage-frequency scaling, C-states deal with CPU sleep states so that some of the CPU functions, which are not necessary at that moment, can be disabled. The P-states and C-states work together to make the processor run more efficiently. It helps the OS and apps determine which cores can be parked and which should be boosted.

Of course not every user is an enthusiast or knows the technicalities and integrities of how things like overclocking or undervolting work. Thankfully for them Windows itself offers something pretty cool, though it is hidden by default on all systems.

By default, Windows only has two P-States, "Minimum Processor State" and "Maximum Processor State." However, this can be changed with a Registry trick to expand the options under a secret "Processor performance boost mode" dropdown. This essentially enables the HWP or hardware P-States available on a device, and these are not controlled just by the OS itself as the underlying hardware gets involved too.

In total there are five Processor Performance Boost Mode profiles that control how Windows requests and allows CPU turbo/boost behavior under the different power policies. They are:

Disabled: In this mode, processor boosting is effectively turned off. The CPU will avoid entering turbo or boost frequencies and instead operate closer to its base frequency ceiling. This can significantly reduce power consumption and heat output, but at the cost of reduced burst performance and responsiveness in short workloads.

Enabled: This is the standard behavior where boost functionality is allowed under normal conditions. The processor can opportunistically increase frequency when workload demands it, balancing performance gains with power and thermal constraints as managed by the system.

Aggressive: Aggressive mode favors performance more heavily, allowing the CPU to enter higher boost states more readily and sustain them longer. This should in theory improve responsiveness under bursty or heavy workloads but increases power draw and thermal output compared to the default enabled behavior.

Efficient Enabled: This mode still allows boosting, but with a stronger bias toward energy efficiency. The system attempts to use boost more selectively, avoiding unnecessary frequency spikes when the performance gain is marginal.

Efficient Aggressive: This is a hybrid approach where boost is still performance-responsive, but the system continuously weighs efficiency more heavily than in Aggressive mode. It aims to deliver noticeable performance improvements while reducing wasted power in less demanding scenarios.

Here's how to enable the Processor performance boost mode:

Open Registry Editor: Press Win+R, type regedit, and click OK.
Go to:

HKLM\SYSTEM\CurrentControlSet\Control\Power\PowerSettings\54533251-82be-4824-96c1-47b60b740d00\be337238-0d82-4146-a960-4f3749d470c7 Windows 11 processor performance boost modes enable (where HKLM stands for HKEY_LOCAL_MACHINE_)
Modify the value of Attributes from 1 to 2 (you can find modify option by right-clicking)
Windows 11 processor performance boost modes enable Windows 11 processor performance boost modes enable
After that, exit Registry, you should now be able to see the new "Processor performance boost mode" dropdown menu:

Windows 11 processor performance boost modes enable
As you can see there are now five new P-States or CPPC states or power profile available that help define the boost mode processor setting on your PC.

Windows 11 processor performance boost modes enable
Wrapping it up here's a quick run-down of the settings as defined by Microsoft itself.

Setting	Description
Disabled	The corresponding P-state-based behaviour is disabled. Collaborative Processor Performance Control (CPPC) behaviour is disabled.
Enabled	The corresponding P-state-based behaviour is enabled. CPPC behaviour is Efficient Enabled.
Aggressive	The corresponding P-state-based behaviour is enabled. CPPC behaviour is Aggressive.
Efficient Enabled	The corresponding P-state-based behaviour is Efficient. CPPC behaviour is Efficient Enabled.
Efficient Aggressive	The corresponding P-state-based behaviour is Efficient. CPPC behaviour is Aggressive.
Aggressive At Guaranteed	Windows calculates the desired extra performance above the guaranteed performance level, and asks the processor to deliver that specific performance level.
Efficient Aggressive At Guaranteed	Windows always asks the processor to deliver the highest possible performance above the guaranteed performance level.
In the next part we shall be comparing these settings to explore how much of a benefit or regression they can provide in terms of performance and power efficiency. If you decide to change the values on your system and are experiencing problems like crashes or an overheating PC, make sure to revert the steps back to the original state.