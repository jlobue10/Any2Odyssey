// SPDX-License-Identifier: MIT
// Calls into Leia's OpenGL weaver for weaver-host.c. The weaver's interface is a C++ class with virtual bases
// in the Microsoft C++ layout, which the MinGW compiler does not produce, so this one file is compiled with
// clang for the MSVC ABI (no headers, no runtime) and the rest of the program stays plain C.
// The declarations repeat the order of the virtual functions in Leia's MIT-licensed SDK headers
// (sr/utility/commoninterfaces.h, sr/weaver/IWeaverBase.h, sr/weaver/glweaver.h).
typedef unsigned long long u64;
namespace SR {
class SRContext;
class IDestroyable {
public:
    virtual void destroy() = 0;
};
class IQueryInterface {
protected:
    virtual void *queryInterface(void *type_index_by_value) = 0; // never called from here
};
class IWeaverBase1 : virtual IQueryInterface {
public:
    virtual void setWindowHandle(void *handle) = 0;
    virtual void weave() = 0;
    virtual void enableLateLatching(bool enable) = 0;
    virtual bool isLateLatchingEnabled() const = 0;
    virtual void setShaderSRGBConversion(bool read, bool write) = 0;
    virtual void setLatencyInFrames(u64 frames) = 0;
    virtual void setLatency(u64 microseconds) = 0;
    virtual u64 getLatency() const = 0;
    virtual void getPredictedEyePositions(float left[3], float right[3]) = 0;
protected:
    virtual ~IWeaverBase1();
};
class IGLWeaver1 : public virtual IDestroyable, public virtual IQueryInterface, public virtual IWeaverBase1 {
public:
    virtual void setInputViewTexture(unsigned texture, int width, int height, unsigned format) = 0;
protected:
    virtual ~IGLWeaver1() = 0;
};
typedef int (*create_fn)(SRContext &, void *, IGLWeaver1 **);
}
extern "C" {
void *a2o_weaver_create(void *create, void *context, void *window, int *error)
{
    SR::IGLWeaver1 *weaver = nullptr;
    *error = ((SR::create_fn)create)(*(SR::SRContext *)context, window, &weaver);
    return weaver;
}
void a2o_weaver_input(void *w, unsigned texture, int width, int height, unsigned format)
{
    ((SR::IGLWeaver1 *)w)->setInputViewTexture(texture, width, height, format);
}
void a2o_weaver_weave(void *w) { ((SR::IGLWeaver1 *)w)->weave(); }
void a2o_weaver_eyes(void *w, float *left, float *right) { ((SR::IGLWeaver1 *)w)->getPredictedEyePositions(left, right); }
void a2o_weaver_destroy(void *w) { ((SR::IGLWeaver1 *)w)->destroy(); }
}
