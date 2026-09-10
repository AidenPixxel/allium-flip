//! The CPU clock: cpufreq for what the kernel offers, the PLL itself for what it does not.

use std::fs::{self, OpenOptions};
use std::ptr;

use anyhow::{Context, Result, bail};
use log::debug;
use memmap2::MmapOptions;

/// The same node ffplay's launch script writes; `policy0` is the other spelling of it
const GOVERNOR: &str = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor";
/// Honoured only under the `userspace` governor
const SETSPEED: &str = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_setspeed";
/// What the device boots at, and the top of its cpufreq table
pub const STOCK_KHZ: u32 = 1_200_000;

/// The SSD202D's MPLL register block: RIU base plus the bank offset, in 16-bit registers
const MPLL_BASE: u64 = 0x1F00_0000 + 0x10_3000 * 2;
/// One page, which the whole block fits in
const PLL_SIZE: usize = 0x1000;
/// How long to wait for the PLL to report its frequency change done before giving up on it
const PLL_SETTLE_POLLS: u32 = 1_000_000;

/// Runs the CPU at `khz`, past what the kernel's own table allows, handing back the governor that
/// was in place so `stock` can put it back.
///
/// cpufreq is told first -- `userspace`, then the speed -- so that what the kernel believes and
/// what the hardware does agree, which is the precondition the original code states. The speed
/// write is refused for anything above the table's 1.2 GHz ceiling; that is expected and not an
/// error. Then the PLL is reprogrammed directly.
pub fn overclock(khz: u32) -> Result<String> {
    let previous = fs::read_to_string(GOVERNOR)
        .context("failed to read the cpufreq governor")?
        .trim()
        .to_owned();
    fs::write(GOVERNOR, "userspace").context("failed to set the cpufreq governor")?;
    if let Err(err) = fs::write(SETSPEED, khz.to_string()) {
        debug!("cpufreq refused {khz} kHz, as it does above its table: {err}");
    }
    if let Err(err) = set_mpll(khz) {
        // Not left half-done: the governor goes back, and the kernel's own clock with it
        let _ = fs::write(GOVERNOR, &previous);
        return Err(err);
    }
    Ok(previous)
}

/// Back to the clock the device boots with, then to the governor `overclock` replaced.
///
/// The PLL is programmed explicitly rather than left to the governor. The driver believes the
/// clock never left 1.2 GHz (it refused the higher speed), and cpufreq skips a target equal to
/// the current one, so restoring `performance` alone would leave the PLL where it was.
pub fn stock(governor: &str) -> Result<()> {
    set_mpll(STOCK_KHZ)?;
    let _ = fs::write(SETSPEED, STOCK_KHZ.to_string());
    fs::write(GOVERNOR, governor).context("failed to restore the cpufreq governor")
}

/// Reprograms the MPLL for `khz`.
///
/// A port of `set_cpuclock` in MinUI's `workspace/miyoomini/overclock/overclock.c`, itself from
/// eggs' `overclock_test`, which has run on this SoC across the Mini, Mini+ and Mini Flip for
/// years. Register indices are 16-bit words from the block's base. The one liberty taken: the
/// original's unbounded loops -- the post-divider walk and the done poll -- are capped, so a PLL
/// that never answers cannot hang the daemon.
fn set_mpll(khz: u32) -> Result<()> {
    let post_div: u32 = match khz {
        800_000.. => 2,
        400_000.. => 4,
        200_000.. => 8,
        _ => 16,
    };
    const DIVSRC: u64 = 432_000_000 * 524_288;
    let rate = khz * 1000 / 16 * post_div / 2;
    let lpf = (DIVSRC / rate as u64) as u32;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/mem")
        .context("failed to open /dev/mem")?;
    // SAFETY: /dev/mem at a fixed physical address is exactly what the PLL block is; nothing else
    // in this process maps it, and the mapping is dropped at the end of this function
    let mut map = unsafe {
        MmapOptions::new()
            .offset(MPLL_BASE)
            .len(PLL_SIZE)
            .map_mut(&file)
    }
    .context("failed to map the PLL registers")?;
    let regs = map.as_mut_ptr() as *mut u16;

    // SAFETY: every index below is well inside the page, and the accesses are volatile because
    // these are hardware registers, not memory
    unsafe {
        let rd = |i: usize| ptr::read_volatile(regs.add(i));
        let wr = |i: usize, v: u16| ptr::write_volatile(regs.add(i), v);

        let cur_post_div = (rd(0x232) & 0x0F) as u32 + 1;
        let mut tmp_post_div = cur_post_div;
        // A larger divider first, so the clock only ever steps down on the way to a new rate
        if post_div > cur_post_div {
            let mut steps = 0;
            while tmp_post_div != post_div {
                tmp_post_div <<= 1;
                wr(
                    0x232,
                    (rd(0x232) & 0xF0) | ((tmp_post_div - 1) & 0x0F) as u16,
                );
                steps += 1;
                if steps > 4 {
                    bail!("post-divider {cur_post_div} cannot reach {post_div}");
                }
            }
        }

        wr(0x2A8, 0x0000); // reg_lpf_enable = 0
        wr(0x2AE, 0x000F); // reg_lpf_update_cnt = 32
        wr(0x2A4, (lpf & 0xFFFF) as u16); // target frequency, LPF high
        wr(0x2A6, (lpf >> 16) as u16);
        wr(0x2B0, 0x0001); // switch to LPF control
        wr(0x2B2, rd(0x2B2) | 0x1000); // from low to high
        wr(0x2A8, 0x0001); // reg_lpf_enable = 1
        let mut polls = 0;
        while (rd(0x2BA) & 1) == 0 {
            polls += 1;
            if polls > PLL_SETTLE_POLLS {
                bail!("the PLL did not report the change to {khz} kHz done");
            }
        }
        wr(0x2A0, (lpf & 0xFFFF) as u16); // store frequency, LPF low
        wr(0x2A2, (lpf >> 16) as u16);

        if post_div < cur_post_div {
            let mut steps = 0;
            while tmp_post_div != post_div {
                tmp_post_div >>= 1;
                wr(
                    0x232,
                    (rd(0x232) & 0xF0) | ((tmp_post_div - 1) & 0x0F) as u16,
                );
                steps += 1;
                if steps > 4 {
                    bail!("post-divider {cur_post_div} cannot reach {post_div}");
                }
            }
        }
    }
    debug!("MPLL programmed for {khz} kHz (lpf {lpf:#x}, post-divider {post_div})");
    Ok(())
}

/// Drops the CPU to the slowest clock the kernel offers, handing back the governor that was in
/// place so `restore` can put it back.
///
/// The kernel boots with `performance`, which pins 1.2 GHz -- including while everything is
/// SIGSTOPped behind a dark panel, where the clock buys nothing and the standby time pays for it.
/// `powersave` is the driver's floor. The driver reprograms the PLL itself on a governor change,
/// so anything set outside the driver has to be put back separately on the way out.
pub fn floor() -> Result<String> {
    let previous = fs::read_to_string(GOVERNOR)
        .context("failed to read the cpufreq governor")?
        .trim()
        .to_owned();
    fs::write(GOVERNOR, "powersave").context("failed to set the cpufreq governor")?;
    Ok(previous)
}

/// Puts back the governor `floor` replaced
pub fn restore(governor: &str) -> Result<()> {
    fs::write(GOVERNOR, governor).context("failed to restore the cpufreq governor")
}
