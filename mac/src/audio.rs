#![allow(non_snake_case, non_upper_case_globals)]
use std::ffi::c_void;

type AudioUnit = *mut c_void;
type OSStatus = i32;

#[repr(C)]
struct AudioComponentDescription {
    componentType: u32,
    componentSubType: u32,
    componentManufacturer: u32,
    componentFlags: u32,
    componentFlagsMask: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AudioStreamBasicDescription {
    mSampleRate: f64,
    mFormatID: u32,
    mFormatFlags: u32,
    mBytesPerPacket: u32,
    mFramesPerPacket: u32,
    mBytesPerFrame: u32,
    mChannelsPerFrame: u32,
    mBitsPerChannel: u32,
    mReserved: u32,
}

#[repr(C)]
struct AudioBuffer {
    mNumberChannels: u32,
    mDataByteSize: u32,
    mData: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    mNumberBuffers: u32,
    mBuffers: [AudioBuffer; 1],
}

type RenderProc = extern "C" fn(*mut c_void, *mut u32, *const c_void, u32, u32, *mut AudioBufferList) -> OSStatus;

#[repr(C)]
struct AURenderCallbackStruct {
    inputProc: RenderProc,
    inputProcRefCon: *mut c_void,
}

#[repr(C)]
struct AudioObjectPropertyAddress {
    mSelector: u32,
    mScope: u32,
    mElement: u32,
}

type PropertyListener = extern "C" fn(u32, u32, *const AudioObjectPropertyAddress, *mut c_void) -> OSStatus;

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

const kAudioUnitProperty_StreamFormat: u32 = 8;
const kAudioUnitProperty_MaximumFramesPerSlice: u32 = 14;
const kAudioUnitProperty_SetRenderCallback: u32 = 23;
const kAudioOutputUnitProperty_CurrentDevice: u32 = 2000;
const kAudioOutputUnitProperty_OSWorkgroup: u32 = 2015;
const THREAD_TIME_CONSTRAINT_POLICY: u32 = 2;
const kAudioDevicePropertyBufferFrameSize: u32 = fourcc(b"fsiz");
const kAudioHardwarePropertyDefaultOutputDevice: u32 = fourcc(b"dOut");
const kAudioObjectPropertyScopeGlobal: u32 = fourcc(b"glob");
const kAudioObjectPropertyScopeOutput: u32 = fourcc(b"outp");
const kAudioObjectPropertyElementMain: u32 = 0;
const kAudioUnitScope_Global: u32 = 0;
const kAudioUnitScope_Input: u32 = 1;
const kAudioUnitScope_Output: u32 = 2;
const kAudioFormatFlagsNonInterleavedFloat: u32 = 1 | 8 | 0x20;
const kAudioObjectSystemObject: u32 = 1;

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioComponentFindNext(after: *mut c_void, desc: *const AudioComponentDescription) -> *mut c_void;
    fn AudioComponentInstanceNew(component: *mut c_void, out: *mut AudioUnit) -> OSStatus;
    fn AudioComponentInstanceDispose(unit: AudioUnit) -> OSStatus;
    fn AudioUnitSetProperty(unit: AudioUnit, id: u32, scope: u32, element: u32, data: *const c_void, size: u32) -> OSStatus;
    fn AudioUnitGetProperty(unit: AudioUnit, id: u32, scope: u32, element: u32, data: *mut c_void, size: *mut u32) -> OSStatus;
    fn AudioUnitInitialize(unit: AudioUnit) -> OSStatus;
    fn AudioUnitUninitialize(unit: AudioUnit) -> OSStatus;
    fn AudioOutputUnitStart(unit: AudioUnit) -> OSStatus;
    fn AudioOutputUnitStop(unit: AudioUnit) -> OSStatus;
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectSetPropertyData(object: u32, address: *const AudioObjectPropertyAddress, qualifier_size: u32, qualifier: *const c_void, size: u32, data: *const c_void) -> OSStatus;
    fn AudioObjectGetPropertyData(object: u32, address: *const AudioObjectPropertyAddress, qualifier_size: u32, qualifier: *const c_void, size: *mut u32, data: *mut c_void) -> OSStatus;
    fn AudioObjectAddPropertyListener(object: u32, address: *const AudioObjectPropertyAddress, listener: PropertyListener, client: *mut c_void) -> OSStatus;
}

unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
    fn pthread_self() -> *mut c_void;
    fn pthread_mach_thread_np(thread: *mut c_void) -> u32;
    fn thread_policy_set(thread: u32, flavor: u32, info: *const c_void, count: u32) -> i32;
    fn mach_timebase_info(info: *mut TimebaseInfo) -> i32;
    fn os_workgroup_join(workgroup: *mut c_void, token: *mut c_void) -> i32;
    fn os_workgroup_leave(workgroup: *mut c_void, token: *mut c_void);
    fn os_workgroup_max_parallel_threads(workgroup: *mut c_void, attr: *const c_void) -> i32;
    fn os_release(object: *mut c_void);
}

const LONGEST_COMPUTATION_NS: f64 = 45e6;

#[repr(C)]
#[derive(Clone, Copy)]
struct TimeConstraintPolicy {
    period: u32,
    computation: u32,
    constraint: u32,
    preemptible: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TimebaseInfo {
    numer: u32,
    denom: u32,
}

struct Workgroup(*mut c_void);

impl Workgroup {
    fn raw(&self) -> *mut c_void {
        self.0
    }
}

unsafe impl Send for Workgroup {}
unsafe impl Sync for Workgroup {}

impl Drop for Workgroup {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { os_release(self.0) };
        }
    }
}

pub type Render = Box<dyn FnMut(&mut [f32], &mut [f32]) + Send>;

extern "C" fn render_proc(ref_con: *mut c_void, _: *mut u32, _: *const c_void, _: u32, frames: u32, data: *mut AudioBufferList) -> OSStatus {
    let render = unsafe { &mut *(ref_con as *mut Render) };
    let n = frames as usize;
    let (left, right) = unsafe {
        if (*data).mNumberBuffers < 2 {
            return 0;
        }
        let buffers = (&raw const (*data).mBuffers).cast::<AudioBuffer>();
        let (b0, b1) = (&*buffers, &*buffers.add(1));
        if (b0.mDataByteSize as usize) < n * 4 || (b1.mDataByteSize as usize) < n * 4 {
            return 0;
        }
        (std::slice::from_raw_parts_mut(b0.mData as *mut f32, n), std::slice::from_raw_parts_mut(b1.mData as *mut f32, n))
    };
    render(left, right);
    0
}

pub struct Output {
    unit: AudioUnit,
    pub sample_rate: f64,
    pub max_frames: usize,
    pub frames: u32,
    render: *mut Render,
}

fn check(status: OSStatus) -> Result<(), OSStatus> {
    if status == 0 { Ok(()) } else { Err(status) }
}

impl Output {
    pub fn open(wanted: u32) -> Result<Output, OSStatus> {
        let desc = AudioComponentDescription { componentType: fourcc(b"auou"), componentSubType: fourcc(b"ahal"), componentManufacturer: fourcc(b"appl"), componentFlags: 0, componentFlagsMask: 0 };
        let component = unsafe { AudioComponentFindNext(std::ptr::null_mut(), &desc) };
        if component.is_null() {
            return Err(-1);
        }
        let mut unit: AudioUnit = std::ptr::null_mut();
        check(unsafe { AudioComponentInstanceNew(component, &mut unit) })?;
        let mut output = Output { unit, sample_rate: 48_000.0, max_frames: 512, frames: wanted, render: std::ptr::null_mut() };
        let mut hardware = AudioStreamBasicDescription::default();
        let mut size = size_of::<AudioStreamBasicDescription>() as u32;
        check(unsafe { AudioUnitGetProperty(unit, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Output, 0, (&mut hardware as *mut AudioStreamBasicDescription).cast(), &mut size) })?;
        if hardware.mSampleRate > 0.0 {
            output.sample_rate = hardware.mSampleRate;
        }
        let format = AudioStreamBasicDescription {
            mSampleRate: output.sample_rate,
            mFormatID: fourcc(b"lpcm"),
            mFormatFlags: kAudioFormatFlagsNonInterleavedFloat,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        check(unsafe { AudioUnitSetProperty(unit, kAudioUnitProperty_StreamFormat, kAudioUnitScope_Input, 0, (&format as *const AudioStreamBasicDescription).cast(), size_of::<AudioStreamBasicDescription>() as u32) })?;
        let mut device: u32 = 0;
        let mut device_size = 4u32;
        if unsafe { AudioUnitGetProperty(unit, kAudioOutputUnitProperty_CurrentDevice, kAudioUnitScope_Global, 0, (&mut device as *mut u32).cast(), &mut device_size) } == 0 && device != 0 {
            let address = AudioObjectPropertyAddress { mSelector: kAudioDevicePropertyBufferFrameSize, mScope: kAudioObjectPropertyScopeOutput, mElement: kAudioObjectPropertyElementMain };
            let _ = unsafe { AudioObjectSetPropertyData(device, &address, 0, std::ptr::null(), 4, (&wanted as *const u32).cast()) };
            let mut given: u32 = 0;
            let mut given_size = 4u32;
            if unsafe { AudioObjectGetPropertyData(device, &address, 0, std::ptr::null(), &mut given_size, (&mut given as *mut u32).cast()) } == 0 && given > 0 {
                output.frames = given;
            }
        }
        let cap: u32 = output.frames.clamp(64, 8192);
        let _ = unsafe { AudioUnitSetProperty(unit, kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitScope_Global, 0, (&cap as *const u32).cast(), 4) };
        let mut max_frames: u32 = 0;
        let mut max_size = 4u32;
        check(unsafe { AudioUnitGetProperty(unit, kAudioUnitProperty_MaximumFramesPerSlice, kAudioUnitScope_Global, 0, (&mut max_frames as *mut u32).cast(), &mut max_size) })?;
        output.max_frames = max_frames.max(512) as usize;
        Ok(output)
    }

    pub fn pool(&self) -> (usize, impl Fn() -> bp::audio::Demote + Send + Sync + 'static) {
        let mut workgroup: *mut c_void = std::ptr::null_mut();
        let mut size = size_of::<*mut c_void>() as u32;
        if unsafe { AudioUnitGetProperty(self.unit, kAudioOutputUnitProperty_OSWorkgroup, kAudioUnitScope_Global, 0, (&mut workgroup as *mut *mut c_void).cast(), &mut size) } != 0 {
            workgroup = std::ptr::null_mut();
        }
        let helpers = if workgroup.is_null() {
            std::thread::available_parallelism().map_or(0, |cores| cores.get().min(4).saturating_sub(1))
        } else {
            (unsafe { os_workgroup_max_parallel_threads(workgroup, std::ptr::null()) }.max(1) as usize).saturating_sub(1)
        };
        let mut timebase = TimebaseInfo::default();
        unsafe { mach_timebase_info(&mut timebase) };
        let scale = if timebase.numer > 0 { f64::from(timebase.denom) / f64::from(timebase.numer) } else { 0.0 };
        let period_ns = f64::from(self.frames) / self.sample_rate * 1e9;
        let ticks = |ns: f64| (ns * scale) as u32;
        let policy = TimeConstraintPolicy { period: ticks(period_ns), computation: ticks((period_ns * 0.5).min(LONGEST_COMPUTATION_NS)), constraint: ticks(period_ns), preemptible: 1 };
        let workgroup = Workgroup(workgroup);
        let promote = move || {
            let refused = unsafe { thread_policy_set(pthread_mach_thread_np(pthread_self()), THREAD_TIME_CONSTRAINT_POLICY, (&policy as *const TimeConstraintPolicy).cast(), 4) };
            if refused != 0 {
                eprintln!("a render helper was refused the time-constraint policy: {refused}");
            }
            if workgroup.raw().is_null() {
                return None;
            }
            let mut token = [0u32; 10];
            if unsafe { os_workgroup_join(workgroup.raw(), token.as_mut_ptr().cast()) } != 0 {
                return None;
            }
            let joined = workgroup.raw() as usize;
            Some(Box::new(move || {
                let mut token = token;
                unsafe { os_workgroup_leave(joined as *mut c_void, token.as_mut_ptr().cast()) };
            }) as Box<dyn FnOnce() + Send>)
        };
        (helpers, promote)
    }

    pub fn start(&mut self, render: Render) -> Result<(), OSStatus> {
        self.render = Box::into_raw(Box::new(render));
        let callback = AURenderCallbackStruct { inputProc: render_proc, inputProcRefCon: self.render.cast() };
        check(unsafe { AudioUnitSetProperty(self.unit, kAudioUnitProperty_SetRenderCallback, kAudioUnitScope_Input, 0, (&callback as *const AURenderCallbackStruct).cast(), size_of::<AURenderCallbackStruct>() as u32) })?;
        check(unsafe { AudioUnitInitialize(self.unit) })?;
        check(unsafe { AudioOutputUnitStart(self.unit) })
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        unsafe {
            AudioOutputUnitStop(self.unit);
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
            if !self.render.is_null() {
                drop(Box::from_raw(self.render));
            }
        }
    }
}

extern "C" fn device_changed(_: u32, _: u32, _: *const AudioObjectPropertyAddress, client: *mut c_void) -> OSStatus {
    unsafe { dispatch_async_f(&_dispatch_main_q, client, on_main) };
    0
}

extern "C" fn on_main(client: *mut c_void) {
    let f: fn() = unsafe { std::mem::transmute(client) };
    f();
}

pub fn watch_default_device(f: fn()) {
    let address = AudioObjectPropertyAddress { mSelector: kAudioHardwarePropertyDefaultOutputDevice, mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMain };
    unsafe { AudioObjectAddPropertyListener(kAudioObjectSystemObject, &address, device_changed, f as *mut c_void) };
}
