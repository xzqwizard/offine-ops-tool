use crate::error::{AppError, AppResult};
use crate::models::Project;
use rust_xlsxwriter::{Format, FormatAlign, FormatBorder, Workbook};

fn io(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::Io(e.to_string())
}

/// 端口矩阵 xlsx 导出（M2）：
/// Sheet1「端口矩阵」：全网每台服务器监听端口与放行来源
/// Sheet2「防火墙开通申请表」：网络组申请格式（源/目标/端口/协议/用途，申请人留空）
#[tauri::command]
pub fn export_port_matrix_xlsx(project: Project, output_path: String) -> AppResult<String> {
    if output_path.trim().is_empty() {
        return Err(AppError::Invalid("未选择保存位置".into()));
    }
    let path = output_path.trim().to_string();
    if !path.to_lowercase().ends_with(".xlsx") {
        return Err(AppError::Invalid("保存路径需以 .xlsx 结尾".into()));
    }

    let mut workbook = Workbook::new();
    let title = Format::new().set_bold().set_font_size(14);
    let header = Format::new()
        .set_bold()
        .set_border(FormatBorder::Thin)
        .set_background_color("#1e2d48")
        .set_font_color("#FFFFFF")
        .set_align(FormatAlign::Center);
    let cell = Format::new().set_border(FormatBorder::Thin);
    let cell_center = cell.clone().set_align(FormatAlign::Center);
    let cell_mono = cell.clone().set_font_name("Consolas");

    // ============ Sheet1 端口矩阵 ============
    let sheet = workbook.add_worksheet();
    sheet.set_name("端口矩阵").map_err(io)?;
    let ts = project
        .updated_at
        .replace('T', " ")
        .split('.')
        .next()
        .unwrap_or("")
        .to_string();
    sheet
        .merge_range(0, 0, 0, 7, &format!("端口矩阵 - {}", project.name), &title)
        .map_err(io)?;
    sheet
        .write_with_format(
            1,
            0,
            format!(
                "生成时间：{ts}　服务器：{} 台　实例：{} 个",
                project.servers.len(),
                project.instances.len()
            ),
            &cell,
        )
        .map_err(io)?;

    let headers = [
        "目标服务器",
        "IP",
        "架构",
        "服务(实例)",
        "端口",
        "协议",
        "容器端口",
        "放行来源",
    ];
    for (i, h) in headers.iter().enumerate() {
        sheet
            .write_with_format(3, i as u16, *h, &header)
            .map_err(io)?;
    }

    let mut row: u32 = 4;
    for server in &project.servers {
        let instances: Vec<_> = project
            .instances
            .iter()
            .filter(|i| i.server_id == server.id)
            .collect();
        let mut wrote_any = false;
        for inst in &instances {
            for port in &inst.ports {
                if !port.expose {
                    continue;
                }
                let sources = project
                    .network_rules
                    .iter()
                    .filter(|r| {
                        r.to_server_id == server.id
                            && r.to_port == port.host
                            && r.protocol == port.protocol
                    })
                    .filter_map(|r| {
                        project
                            .servers
                            .iter()
                            .find(|s| s.id == r.from_server_id)
                            .map(|s| {
                                format!("{}({})", s.name, if s.ip.is_empty() { "?" } else { &s.ip })
                            })
                    })
                    .collect::<Vec<_>>()
                    .join("、");
                sheet
                    .write_with_format(
                        row,
                        0,
                        if wrote_any { "" } else { server.name.as_str() },
                        &cell,
                    )
                    .map_err(io)?;
                sheet
                    .write_with_format(
                        row,
                        1,
                        if wrote_any { "" } else { server.ip.as_str() },
                        &cell_mono,
                    )
                    .map_err(io)?;
                sheet
                    .write_with_format(
                        row,
                        2,
                        if wrote_any { "" } else { server.arch.as_str() },
                        &cell_center,
                    )
                    .map_err(io)?;
                sheet
                    .write_with_format(row, 3, inst.instance_name.as_str(), &cell)
                    .map_err(io)?;
                sheet
                    .write_with_format(row, 4, port.host, &cell_mono)
                    .map_err(io)?;
                sheet
                    .write_with_format(row, 5, port.protocol.as_str(), &cell_center)
                    .map_err(io)?;
                sheet
                    .write_with_format(row, 6, port.container, &cell_mono)
                    .map_err(io)?;
                sheet
                    .write_with_format(row, 7, sources.as_str(), &cell)
                    .map_err(io)?;
                row += 1;
                wrote_any = true;
            }
        }
        if !wrote_any {
            sheet
                .write_with_format(row, 0, server.name.as_str(), &cell)
                .map_err(io)?;
            sheet
                .write_with_format(row, 1, server.ip.as_str(), &cell_mono)
                .map_err(io)?;
            sheet
                .write_with_format(row, 2, server.arch.as_str(), &cell_center)
                .map_err(io)?;
            sheet
                .write_with_format(row, 3, "（无对外端口）", &cell)
                .map_err(io)?;
            for col in 4..8u16 {
                sheet.write_with_format(row, col, "", &cell).map_err(io)?;
            }
            row += 1;
        }
    }
    sheet.set_column_width(0, 18).map_err(io)?;
    sheet.set_column_width(1, 14).map_err(io)?;
    sheet.set_column_width(2, 10).map_err(io)?;
    sheet.set_column_width(3, 16).map_err(io)?;
    sheet.set_column_width(7, 40).map_err(io)?;

    // ============ Sheet2 防火墙开通申请表 ============
    let sheet2 = workbook.add_worksheet();
    sheet2.set_name("防火墙开通申请").map_err(io)?;
    sheet2
        .merge_range(
            0,
            0,
            0,
            7,
            &format!("防火墙端口开通申请表 - {}", project.name),
            &title,
        )
        .map_err(io)?;
    sheet2
        .write_with_format(
            1,
            0,
            "说明：一行一条规则；申请人/审批人/日期由双方填写后归档",
            &cell,
        )
        .map_err(io)?;

    let headers2 = [
        "序号",
        "源服务器",
        "源地址",
        "目标服务器",
        "目标地址",
        "端口",
        "协议",
        "用途说明",
    ];
    for (i, h) in headers2.iter().enumerate() {
        sheet2
            .write_with_format(3, i as u16, *h, &header)
            .map_err(io)?;
    }
    let mut row: u32 = 4;
    let mut seq = 1u32;
    for rule in &project.network_rules {
        let from = project.servers.iter().find(|s| s.id == rule.from_server_id);
        let to = project.servers.iter().find(|s| s.id == rule.to_server_id);
        let (Some(from), Some(to)) = (from, to) else {
            continue;
        };
        sheet2
            .write_with_format(row, 0, seq, &cell_center)
            .map_err(io)?;
        sheet2
            .write_with_format(row, 1, from.name.as_str(), &cell)
            .map_err(io)?;
        sheet2
            .write_with_format(row, 2, from.ip.as_str(), &cell_mono)
            .map_err(io)?;
        sheet2
            .write_with_format(row, 3, to.name.as_str(), &cell)
            .map_err(io)?;
        sheet2
            .write_with_format(row, 4, to.ip.as_str(), &cell_mono)
            .map_err(io)?;
        sheet2
            .write_with_format(row, 5, rule.to_port, &cell_mono)
            .map_err(io)?;
        sheet2
            .write_with_format(row, 6, rule.protocol.as_str(), &cell_center)
            .map_err(io)?;
        sheet2
            .write_with_format(row, 7, rule.description.as_str(), &cell)
            .map_err(io)?;
        row += 1;
        seq += 1;
    }
    if seq == 1 {
        sheet2
            .merge_range(4, 0, 4, 7, "（方案中未定义跨服务器访问规则）", &cell_center)
            .map_err(io)?;
    }
    for (col, w) in [(0u16, 6u16), (1, 16), (2, 14), (3, 16), (4, 14), (7, 36)] {
        sheet2.set_column_width(col, w).map_err(io)?;
    }

    workbook
        .save(&path)
        .map_err(|e| AppError::Io(format!("xlsx 写入失败: {e}")))?;
    Ok(path)
}
