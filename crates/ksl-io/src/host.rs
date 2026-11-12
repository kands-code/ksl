//! 内置宿主资源与值的映射.
//!
//! 内置资源实现 `ksl_value::value::NativeOpaque`, 与插件资源共用 `Value::NativeOpaque`.
//! 每种资源以本模块的包装类型区分; 值的构造与取回按资源分组,
//! 避免值类型反向依赖 I/O 服务.

use std::sync::Arc;

use jiff::ZonedDateTime;
use ksl_value::value::{NativeOpaque, Value};

use crate::io::{CommandOutput, FileHandle, SocketHandle, TcpListenerHandle};
use crate::regex::{CompiledRegex, MatchRecord};

macro_rules! resource {
    ($wrapper:ident, $inner:ty, $name:literal, $ctor:ident, $getter:ident) => {
        /// 宿主资源包装; 只用于在值上区分资源类别.
        pub struct $wrapper(pub Arc<$inner>);

        impl NativeOpaque for $wrapper {
            fn type_name(&self) -> &'static str {
                $name
            }
        }

        #[doc = concat!("以 `", $name, "` 构造值.")]
        #[must_use]
        pub fn $ctor(handle: Arc<$inner>) -> Value {
            Value::native_opaque($wrapper(handle))
        }

        #[doc = concat!("取 `", $name, "` 资源.")]
        #[must_use]
        pub fn $getter(value: &Value) -> Option<&Arc<$inner>> {
            value.as_opaque::<$wrapper>().map(|wrapper| &wrapper.0)
        }
    };
}

resource!(FileResource, FileHandle, "File", file, as_file);
resource!(
    CommandOutputResource,
    CommandOutput,
    "CommandOutput",
    command_output,
    as_command_output
);
resource!(SocketResource, SocketHandle, "Socket", socket, as_socket);
resource!(
    TcpListenerResource,
    TcpListenerHandle,
    "TcpListener",
    tcp_listener,
    as_tcp_listener
);
resource!(
    DateTimeResource,
    ZonedDateTime,
    "DateTime",
    date_time,
    as_date_time
);
resource!(RegexResource, CompiledRegex, "Regex", regex, as_regex);
resource!(
    RegexMatchResource,
    MatchRecord,
    "RegexMatch",
    regex_match,
    as_regex_match
);
