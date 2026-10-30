use super::{State, MAX_BUFFERS_PER_CLIENT, MAX_DMABUF_FEEDBACKS_PER_CLIENT, MAX_DMABUF_PARAMS_PER_CLIENT};
use crate::dmabuf::{DmabufFeedback, DmabufParams, ParamsError};
use std::os::fd::AsFd;
use std::sync::Arc;
use wayland_protocols::wp::linux_dmabuf::zv1::server::{
    zwp_linux_buffer_params_v1, zwp_linux_dmabuf_feedback_v1, zwp_linux_dmabuf_v1,
};
use wayland_server::protocol::wl_buffer;
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

impl GlobalDispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for State {
    fn bind(
        _state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

fn send_feedback(feedback: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1, source: &DmabufFeedback) {
    feedback.format_table(source.table.as_fd(), source.table_size);
    feedback.main_device(source.device_bytes());
    feedback.tranche_target_device(source.device_bytes());
    feedback.tranche_formats(DmabufFeedback::tranche_indices());
    feedback.tranche_flags(zwp_linux_dmabuf_feedback_v1::TrancheFlags::Scanout);
    feedback.tranche_done();
    feedback.done();
}

impl Dispatch<zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1, ()> for State {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
        request: zwp_linux_dmabuf_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwp_linux_dmabuf_v1::Request::CreateParams { params_id } => {
                if state.count_client_objects(state.dmabuf_params.keys(), &client.id()) >= MAX_DMABUF_PARAMS_PER_CLIENT {
                    resource.post_error(0u32, "too many dmabuf params for client");
                    return;
                }
                let params = data_init.init(params_id, ());
                state.dmabuf_params.insert(params.id(), DmabufParams::default());
            }
            zwp_linux_dmabuf_v1::Request::GetDefaultFeedback { id }
            | zwp_linux_dmabuf_v1::Request::GetSurfaceFeedback { id, .. } => {
                if state.count_client_objects(state.dmabuf_feedbacks.iter(), &client.id()) >= MAX_DMABUF_FEEDBACKS_PER_CLIENT {
                    resource.post_error(0u32, "too many dmabuf feedback objects for client");
                    return;
                }
                let feedback = data_init.init(id, ());
                state.dmabuf_feedbacks.insert(feedback.id());
                match &state.dmabuf_feedback {
                    Some(source) => send_feedback(&feedback, source),
                    None => resource.post_error(0u32, "dmabuf feedback unavailable"),
                }
            }
            zwp_linux_dmabuf_v1::Request::Destroy => {}
            _ => {}
        }
    }
}

impl Dispatch<zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1, ()> for State {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        _request: zwp_linux_dmabuf_feedback_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &zwp_linux_dmabuf_feedback_v1::ZwpLinuxDmabufFeedbackV1,
        _data: &(),
    ) {
        state.dmabuf_feedbacks.remove(&resource.id());
    }
}

fn raw_flags(flags: wayland_server::WEnum<zwp_linux_buffer_params_v1::Flags>) -> u32 {
    match flags {
        wayland_server::WEnum::Value(flags) => flags.bits(),
        wayland_server::WEnum::Unknown(raw) => raw,
    }
}

enum CreateTarget {
    Event,
    Immediate(New<wl_buffer::WlBuffer>),
}

fn create_buffer(
    state: &mut State,
    client: &Client,
    dh: &DisplayHandle,
    resource: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
    data_init: &mut DataInit<'_, State>,
    target: CreateTarget,
    (width, height, format, flags): (i32, i32, u32, u32),
) {
    let Some(params) = state.dmabuf_params.get_mut(&resource.id()) else {
        resource.post_error(ParamsError::AlreadyUsed as u32, "unknown dmabuf params");
        return;
    };
    let created = match params.create(width, height, format, flags) {
        Ok(created) => created,
        Err(error) => {
            resource.post_error(error as u32, error.message());
            return;
        }
    };
    let data = match created {
        Ok(data) => data,
        Err(unsupported) => {
            eprintln!("TINYLAND_DMABUF_UNSUPPORTED {unsupported:?}");
            match target {
                CreateTarget::Event => resource.failed(),
                CreateTarget::Immediate(_) => resource.post_error(
                    zwp_linux_buffer_params_v1::Error::InvalidWlBuffer as u32,
                    format!("unsupported dmabuf {unsupported:?}"),
                ),
            }
            return;
        }
    };
    if state.count_client_buffers(&client.id()) >= MAX_BUFFERS_PER_CLIENT {
        resource.post_error(0u32, "too many buffers for client");
        return;
    }
    let buffer = match target {
        CreateTarget::Immediate(id) => data_init.init(id, ()),
        CreateTarget::Event => match client.create_resource::<wl_buffer::WlBuffer, (), State>(dh, 1, ()) {
            Ok(buffer) => {
                resource.created(&buffer);
                buffer
            }
            Err(error) => {
                eprintln!("dmabuf wl_buffer create failed: {error}");
                resource.failed();
                return;
            }
        },
    };
    state.dmabuf_buffers.insert(buffer.id(), (buffer, Arc::new(data)));
}

impl Dispatch<zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1, ()> for State {
    fn request(
        state: &mut Self,
        client: &Client,
        resource: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        request: zwp_linux_buffer_params_v1::Request,
        _data: &(),
        dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwp_linux_buffer_params_v1::Request::Add { fd, plane_idx, offset, stride, modifier_hi, modifier_lo } => {
                let modifier = (modifier_hi as u64) << 32 | modifier_lo as u64;
                let Some(params) = state.dmabuf_params.get_mut(&resource.id()) else { return };
                if let Err(error) = params.add(fd, plane_idx, offset, stride, modifier) {
                    resource.post_error(error as u32, error.message());
                }
            }
            zwp_linux_buffer_params_v1::Request::Create { width, height, format, flags } => {
                let flags = raw_flags(flags);
                create_buffer(state, client, dh, resource, data_init, CreateTarget::Event, (width, height, format, flags));
            }
            zwp_linux_buffer_params_v1::Request::CreateImmed { buffer_id, width, height, format, flags } => {
                let flags = raw_flags(flags);
                create_buffer(state, client, dh, resource, data_init, CreateTarget::Immediate(buffer_id), (width, height, format, flags));
            }
            zwp_linux_buffer_params_v1::Request::Destroy => {}
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: wayland_server::backend::ClientId,
        resource: &zwp_linux_buffer_params_v1::ZwpLinuxBufferParamsV1,
        _data: &(),
    ) {
        state.dmabuf_params.remove(&resource.id());
    }
}
